//! Spike window: `egui-winit` + `egui-wgpu` render integration.
//!
//! `SpikeWindow` encapsulates the winit `Arc<Window>`, wgpu device/queue/surface,
//! and the `EguiRenderer`. It is created inside `ApplicationHandler::resumed()` on
//! the main OS thread (Pitfall 2: wgpu surface must be created on the window's thread).
//!
//! The spike UI (D-07/D-09) is intentionally minimal:
//!   - a text-edit field bound to `self.input`
//!   - an "Aplicar" button that copies `input` into `self.applied`
//!   - a label showing "Aplicado: {applied}"
//!
//! This is the spike stub, NOT the activation screen (Phase 2).

use std::sync::Arc;

use anyhow::Context as _;
use egui_wgpu::ScreenDescriptor;
use winit::{
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::Window,
};

// ── EguiRenderer ─────────────────────────────────────────────────────────────

/// Encapsulates the egui context + egui-winit state + egui-wgpu renderer.
///
/// Follows RESEARCH Pattern 2 closely, adapted for egui 0.35 API:
/// - egui 0.35 uses `begin_pass(raw_input)` + `end_pass()` instead of `ctx.run()`
/// - `egui_wgpu::Renderer::new` takes `RendererOptions` (not positional bool/int args)
/// - `egui_winit::State::new` takes `winit::window::Theme` (not `egui::Theme`)
struct EguiRenderer {
    context: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
}

impl EguiRenderer {
    fn new(
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
        window: &Arc<Window>,
    ) -> Self {
        let context = egui::Context::default();
        let viewport_id = context.viewport_id();
        let state = egui_winit::State::new(
            context.clone(),
            viewport_id,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            Some(winit::window::Theme::Dark), // egui 0.35 takes winit::window::Theme
            None,                             // max_texture_side: use default
        );
        let renderer = egui_wgpu::Renderer::new(
            device,
            output_format,
            egui_wgpu::RendererOptions::default(),
        );
        Self { context, state, renderer }
    }

    /// Forward a winit `WindowEvent` into egui.
    fn handle_input(
        &mut self,
        window: &Window,
        event: &WindowEvent,
    ) -> egui_winit::EventResponse {
        self.state.on_window_event(window, event)
    }

    /// Run the egui UI closure and issue the wgpu render pass.
    ///
    /// Uses the egui 0.35 `run_ui(raw_input, |ui| {...})` API.
    /// The `run_ui` callback receives `&mut egui::Ui` so panels can be shown normally.
    fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &Window,
        window_surface_view: &wgpu::TextureView,
        screen_descriptor: ScreenDescriptor,
        run_ui: impl FnMut(&mut egui::Ui),
    ) {
        let raw_input = self.state.take_egui_input(window);

        // egui 0.35: run_ui runs the UI closure and returns FullOutput.
        let full_output = self.context.run_ui(raw_input, run_ui);

        self.state
            .handle_platform_output(window, full_output.platform_output);

        let primitives = self
            .context
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }

        self.renderer
            .update_buffers(device, queue, encoder, &primitives, &screen_descriptor);

        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: window_surface_view,
                    depth_slice: None, // required field in wgpu 29 (for 3D texture slices)
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
            self.renderer
                .render(&mut render_pass.forget_lifetime(), &primitives, &screen_descriptor);
        }

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}

// ── SpikeWindow ──────────────────────────────────────────────────────────────

/// The walking-skeleton window: winit window + wgpu context + egui renderer + spike UI state.
pub struct SpikeWindow {
    window: Arc<Window>,

    // wgpu context
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,

    // egui renderer
    egui_renderer: EguiRenderer,

    // Spike UI state (D-07/D-09)
    input: String,
    applied: String,
}

impl SpikeWindow {
    /// Create the window, initialise wgpu inside `resumed()` (Pitfall 2), and build the egui renderer.
    ///
    /// The wgpu surface texture format is queried from the adapter capabilities (OQ2).
    pub fn new(event_loop: &ActiveEventLoop) -> anyhow::Result<Self> {
        // ── Create the winit window ──────────────────────────────────────────
        let attrs = Window::default_attributes()
            .with_title("BrevlyPrint — Spike Window")
            .with_inner_size(winit::dpi::LogicalSize::new(640u32, 480u32))
            .with_visible(false); // hidden until wgpu + egui are ready
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .context("Failed to create winit window")?,
        );

        // ── wgpu: Instance ───────────────────────────────────────────────────
        // wgpu::Instance implements Default (picks all available backends).
        let instance = wgpu::Instance::default();

        // ── wgpu: Surface (Pitfall 2: must be on the window thread) ─────────
        // SAFETY: the window lives in an Arc and is kept alive for the duration
        // of the surface. The `Arc::clone` below keeps the count > 0 while surface exists.
        let surface = instance
            .create_surface(Arc::clone(&window))
            .context("Failed to create wgpu surface")?;

        // ── wgpu: Adapter ────────────────────────────────────────────────────
        // Use tokio single-threaded runtime for the async wgpu init calls.
        // tokio is already a project dependency (D-19); no extra crate needed.
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .context("Failed to build tokio runtime for wgpu init")?;

        let adapter = rt
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .context("Failed to find a wgpu adapter (no compatible GPU / Vulkan / DX12 / WARP?)")?;

        // ── wgpu: Device + Queue ─────────────────────────────────────────────
        // wgpu 29: request_device takes only &DeviceDescriptor (no trace arg).
        let (device, queue) = rt
            .block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("brevly_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                ..Default::default()
            }))
            .context("Failed to create wgpu device")?;

        // ── wgpu: Surface configuration ──────────────────────────────────────
        let size = window.inner_size();
        // OQ2 RESOLVED: use get_default_config which picks formats[0] from capabilities.
        let surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("Surface is not supported by the adapter")?;
        let surface_format = surface_config.format;
        surface.configure(&device, &surface_config);

        // ── egui renderer ────────────────────────────────────────────────────
        let egui_renderer = EguiRenderer::new(&device, surface_format, &window);

        // Show window now that everything is ready.
        window.set_visible(true);
        println!("[brevly-print] Window opened (format={surface_format:?})");

        Ok(Self {
            window,
            device,
            queue,
            surface,
            surface_config,
            egui_renderer,
            input: String::new(),
            applied: String::new(),
        })
    }

    /// Get a reference to the underlying `winit::Window`.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Handle a winit `WindowEvent` — forward to egui-winit.
    pub fn handle_input(&mut self, event: &WindowEvent) -> egui_winit::EventResponse {
        self.egui_renderer.handle_input(&self.window, event)
    }

    /// Resize the wgpu surface.
    pub fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }

    /// Render one egui frame onto the wgpu surface.
    pub fn draw(&mut self) -> anyhow::Result<()> {
        // wgpu 29: get_current_texture() returns CurrentSurfaceTexture (enum, not Result).
        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                // Reconfigure for optimal performance next frame but use this texture now.
                self.surface.configure(&self.device, &self.surface_config);
                t
            }
            wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Lost => {
                // Reconfigure and skip this frame.
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
                // Skip silently.
                return Ok(());
            }
        };

        let surface_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame_encoder"),
            });

        // Clear the surface with a dark background before egui renders.
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.1,
                            g: 0.1,
                            b: 0.1,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                ..Default::default()
            });
        }

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        // Spike UI closure (D-07/D-09): text field + button + label.
        // The closure receives `&mut egui::Ui` (from run_ui API — egui 0.35).
        // CentralPanel::show(ui, ...) is used to fill the whole window.
        let input = &mut self.input;
        let applied = &mut self.applied;
        self.egui_renderer.draw(
            &self.device,
            &self.queue,
            &mut encoder,
            &self.window,
            &surface_view,
            screen_descriptor,
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.heading("BrevlyPrint — Walking Skeleton");
                    ui.separator();
                    ui.label("Digite algo:");
                    ui.text_edit_singleline(input);
                    if ui.button("Aplicar").clicked() {
                        *applied = input.clone();
                    }
                    ui.label(format!("Aplicado: {applied}"));
                });
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        Ok(())
    }
}
