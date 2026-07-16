//! Activation window: egui-winit + egui-wgpu render integration.
//!
//! `ActivationWindow` is grown from `spike_window.rs`. It encapsulates:
//!   - The winit `Arc<Window>` + wgpu device/queue/surface (copied verbatim from spike)
//!   - The `EguiRenderer` (copied verbatim from spike)
//!   - An `ActivationFormState` holding all UI state
//!   - A `tokio::runtime::Handle` for spawning async HTTP tasks (Pattern 2)
//!   - A shared `reqwest::Client` (Pitfall 6)
//!   - The app_dir path for the save flow
//!
//! Window attributes (02-UI-SPEC.md Window Geometry):
//!   - Title: "Brevly Print — Ativação"
//!   - Size: 440 × 520 logical points, non-resizable
//!
//! Save flow (D-15, Pitfall 8):
//!   All persistence (DPAPI + SQLite + autostart) happens synchronously BEFORE process exit.

use std::{path::PathBuf, sync::Arc};

use anyhow::Context as _;
use egui::{
    Color32, Frame, RichText, TextStyle, vec2,
};
use egui_wgpu::ScreenDescriptor;
use winit::{
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::Window,
};

use crate::{
    activation_state::{ActivationFormState, FlowState},
    config_store,
    credential_store::{credential_store, CredentialStore as _},
    machine_id,
    noren_client::{self, ActivateError, noren_base_url},
    printer::{printer_from_entry, PrinterId},
};

// ── EguiRenderer ─────────────────────────────────────────────────────────────
// Copied verbatim from spike_window.rs (Phase 1 scaffold).

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

    fn handle_input(
        &mut self,
        window: &Window,
        event: &WindowEvent,
    ) -> egui_winit::EventResponse {
        self.state.on_window_event(window, event)
    }

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
                    depth_slice: None,
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

// ── ActivationWindow ─────────────────────────────────────────────────────────

/// The activation form window: winit window + wgpu context + egui renderer + activation state.
pub struct ActivationWindow {
    window: Arc<Window>,

    // wgpu context (copied verbatim from spike_window.rs)
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,

    // egui renderer
    egui_renderer: EguiRenderer,

    // Activation form state
    state: ActivationFormState,

    // Async runtime handle (Pattern 2 / Pitfall 3)
    rt: tokio::runtime::Handle,

    // Shared HTTP client (Pitfall 6)
    http: reqwest::Client,

    // App directory for save flow
    app_dir: PathBuf,

    // Set to true by the save flow so main.rs can call event_loop.exit()
    should_exit: bool,
}

impl ActivationWindow {
    /// Create the activation window and wgpu + egui context.
    ///
    /// Must be called inside `ApplicationHandler::resumed()` (Pitfall 2: surface on window thread).
    pub fn new(
        event_loop: &ActiveEventLoop,
        rt: tokio::runtime::Handle,
        http: reqwest::Client,
        is_reactivation: bool,
        app_dir: PathBuf,
    ) -> anyhow::Result<Self> {
        // ── Create the winit window (02-UI-SPEC.md Window Geometry) ──────────
        let attrs = Window::default_attributes()
            .with_title("Brevly Print — Ativação")
            .with_inner_size(winit::dpi::LogicalSize::new(440u32, 520u32))
            .with_resizable(false)
            .with_visible(false);
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .context("Failed to create winit window")?,
        );

        // ── wgpu: Instance (copied from spike_window.rs) ─────────────────────
        let instance = wgpu::Instance::default();

        // ── wgpu: Surface (Pitfall 2: must be on the window thread) ─────────
        let surface = instance
            .create_surface(Arc::clone(&window))
            .context("Failed to create wgpu surface")?;

        // ── wgpu: Adapter (use the persistent multi-thread runtime) ──────────
        let adapter = rt
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::default(),
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))
            .context("Failed to find a wgpu adapter")?;

        // ── wgpu: Device + Queue ─────────────────────────────────────────────
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
        let surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("Surface is not supported by the adapter")?;
        let surface_format = surface_config.format;
        surface.configure(&device, &surface_config);

        // ── egui renderer ────────────────────────────────────────────────────
        let egui_renderer = EguiRenderer::new(&device, surface_format, &window);

        // Apply brand colors (02-UI-SPEC.md Palette + Applying Visuals)
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(26, 26, 26);     // #1A1A1A background
        visuals.window_fill = Color32::from_rgb(26, 26, 26);
        visuals.faint_bg_color = Color32::from_rgb(38, 38, 38); // #262626 widget fill
        visuals.override_text_color = Some(Color32::from_rgb(245, 245, 245)); // #F5F5F5
        egui_renderer.context.set_visuals(visuals);

        // Apply typography (02-UI-SPEC.md Typography)
        // egui 0.35 uses global_style_mut (not style_mut).
        egui_renderer.context.global_style_mut(|style| {
            use egui::{FontFamily, FontId};
            style.text_styles = [
                (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
                (TextStyle::Body,    FontId::new(15.0, FontFamily::Proportional)),
                (TextStyle::Button,  FontId::new(15.0, FontFamily::Proportional)),
                (TextStyle::Small,   FontId::new(13.0, FontFamily::Proportional)),
                (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
            ]
            .into();
            // Touch target minimum 36 pt (02-UI-SPEC.md Spacing)
            style.spacing.interact_size.y = 36.0;
        });

        // Show window now that everything is ready.
        window.set_visible(true);
        println!("[brevly-print] Activation window opened (format={surface_format:?})");

        Ok(Self {
            window,
            device,
            queue,
            surface,
            surface_config,
            egui_renderer,
            state: ActivationFormState::new(is_reactivation),
            rt,
            http,
            app_dir,
            should_exit: false,
        })
    }

    /// Get a reference to the underlying `winit::Window`.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// True after the save flow completes — tells main.rs to exit the event loop.
    pub fn should_exit(&self) -> bool {
        self.should_exit
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
    ///
    /// `conn` is the SQLite connection for the save flow `config_store::set` calls.
    pub fn draw(&mut self, conn: &rusqlite::Connection) -> anyhow::Result<()> {
        // ── Surface texture (copied verbatim from spike_window.rs) ───────────
        let surface_texture = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => {
                self.surface.configure(&self.device, &self.surface_config);
                t
            }
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.surface_config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => {
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

        // Clear pass — #1A1A1A (26/255 ≈ 0.102)
        {
            let _clear_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.102,
                            g: 0.102,
                            b: 0.102,
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

        // ── egui UI closure ───────────────────────────────────────────────────
        // Mutable state fields borrowed for the closure.
        let state = &mut self.state;
        let rt = &self.rt;
        let http = &self.http;
        let app_dir = &self.app_dir;
        let should_exit = &mut self.should_exit;

        self.egui_renderer.draw(
            &self.device,
            &self.queue,
            &mut encoder,
            &self.window,
            &surface_view,
            screen_descriptor,
            |ui| {
                // ── Poll async activation result (Pattern 2) ──────────────────
                poll_activate_result(state, ui.ctx());

                // ── Main content panel ────────────────────────────────────────
                egui::CentralPanel::default().show(ui, |ui| {
                    // Outer margin: 32 pt left/right (02-UI-SPEC.md Spacing)
                    // egui 0.35: Margin::symmetric takes i8; use from() for f32.
                    Frame::new()
                        .inner_margin(egui::Margin::symmetric(32i8, 32i8))
                        .show(ui, |ui| {
                            render_form(ui, state, rt, http, app_dir, conn, should_exit);
                        });
                });
            },
        );

        self.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        Ok(())
    }
}

// ── Async result polling ──────────────────────────────────────────────────────

/// Poll the in-flight oneshot receiver each frame (Pattern 2 / Code Examples).
fn poll_activate_result(state: &mut ActivationFormState, ctx: &egui::Context) {
    use tokio::sync::oneshot::error::TryRecvError;

    let result = if let Some(rx) = &mut state.activate_rx {
        match rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint();
                return;
            }
            Err(TryRecvError::Closed) => {
                state.is_busy = false;
                state.activate_rx = None;
                state.serial_error =
                    Some("Sem conexão com o servidor — verifique a internet.".into());
                return;
            }
        }
    } else {
        return;
    };

    state.activate_rx = None;
    state.is_busy = false;

    match result {
        Some(Ok(response)) => {
            state.agent_token = Some(response.agent_token);
            state.tenant_id = Some(response.tenant_id);
            state.enabled_types = response.enabled_types;
            state.pusher_key = Some(response.pusher_key);
            state.pusher_cluster = Some(response.pusher_cluster);
            state.flow = FlowState::ValidatedAwaitingTestPrint;
            ctx.request_repaint();
        }
        Some(Err(ActivateError::InvalidSerial)) => {
            state.serial_error =
                Some("Serial inválido. Verifique o código e tente de novo.".into());
        }
        Some(Err(ActivateError::AlreadyActiveOther)) => {
            state.show_rebind_confirm = true;
        }
        Some(Err(ActivateError::Transport(_))) => {
            state.serial_error =
                Some("Sem conexão com o servidor — verifique a internet.".into());
        }
        None => {
            // No result yet (shouldn't reach here due to early return above).
        }
    }
}

// ── Form rendering ────────────────────────────────────────────────────────────

/// Render the activation form (02-UI-SPEC.md Layout: Single Screen).
#[allow(clippy::too_many_arguments)]
fn render_form(
    ui: &mut egui::Ui,
    state: &mut ActivationFormState,
    rt: &tokio::runtime::Handle,
    http: &reqwest::Client,
    app_dir: &std::path::Path,
    conn: &rusqlite::Connection,
    should_exit: &mut bool,
) {
    // ── Header ────────────────────────────────────────────────────────────────
    ui.add(egui::Label::new(
        RichText::new("Brevly Print").text_style(TextStyle::Heading),
    ));
    ui.add_space(8.0); // sm
    ui.separator();

    // ── Re-activation banner (D-11 — conditional) ────────────────────────────
    if state.is_reactivation {
        ui.add_space(16.0); // md
        Frame::group(ui.style())
            .fill(Color32::from_rgb(38, 38, 38))
            .inner_margin(12i8) // 12 pt — epaint Margin accepts i8
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Precisamos reativar este computador — sua licença continua válida.",
                    )
                    .text_style(TextStyle::Small)
                    .color(Color32::from_rgb(163, 163, 163)),
                );
            });
    }

    // ── Serial field ──────────────────────────────────────────────────────────
    ui.add_space(16.0); // md
    ui.label(RichText::new("Serial").text_style(TextStyle::Body));
    ui.add_space(4.0); // xs

    // WR-03: use TextEdit response.changed() instead of cloning the serial string every frame.
    let serial_response = ui.add(
        egui::TextEdit::singleline(&mut state.serial_input)
            .hint_text("Cole ou digite o serial")
            .desired_width(f32::INFINITY),
    );
    if serial_response.changed() {
        state.on_serial_changed();
    }

    // Enter in serial field triggers "Ativar" (02-UI-SPEC.md Keyboard & Focus).
    let enter_in_serial = serial_response.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter));

    // Inline serial error area (reserve space so layout does not shift).
    ui.add_space(4.0); // xs
    if let Some(err) = &state.serial_error.clone() {
        ui.label(
            RichText::new(err)
                .text_style(TextStyle::Small)
                .color(Color32::from_rgb(239, 68, 68)),
        );
        // "Tentar de novo" button for transport errors (D-12).
        if err.contains("conexão") {
            if ui.button("Tentar de novo").clicked() {
                state.serial_error = None;
            }
        }
    } else {
        // Reserve 18 pt height so layout does not shift on error appearance.
        ui.allocate_space(vec2(ui.available_width(), 18.0));
    }

    // ── 409 Re-bind confirmation (D-02) ──────────────────────────────────────
    if state.show_rebind_confirm {
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "Este serial já está ativo em outro computador. Migrar a licença para esta máquina?",
            )
            .text_style(TextStyle::Body),
        );
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui.button("Manter atual").clicked() {
                state.show_rebind_confirm = false;
                state.serial_error = None;
            }
            // Destructive red "Confirmar migração" (D-02)
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Confirmar migração")
                            .text_style(TextStyle::Button)
                            .color(Color32::WHITE),
                    )
                    .fill(Color32::from_rgb(239, 68, 68)),
                )
                .clicked()
            {
                state.show_rebind_confirm = false;
                // CR-03: force_rebind=true signals Noren this is a confirmed migration.
                dispatch_activate(state, rt, http, true);
            }
        });
        ui.add_space(8.0);
    }

    // ── Printer ComboBox or empty state ──────────────────────────────────────
    ui.add_space(16.0); // md
    ui.label(RichText::new("Impressora").text_style(TextStyle::Body));
    ui.add_space(4.0); // xs

    if state.printer_list.is_empty() {
        // Empty printer state (D-07)
        Frame::group(ui.style())
            .fill(Color32::from_rgb(38, 38, 38))
            .inner_margin(12i8)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Nenhuma impressora encontrada — ligue a impressora e conecte o cabo.",
                    )
                    .text_style(TextStyle::Small)
                    .color(Color32::from_rgb(163, 163, 163)),
                );
            });
        ui.add_space(8.0);
        if ui
            .add(
                egui::Button::new("Atualizar lista")
                    .min_size(vec2(ui.available_width(), 36.0)),
            )
            .clicked()
        {
            state.refresh_printers();
        }
    } else {
        // WR-04: collect only display names before the closure to avoid cloning the
        // entire printer_list (which contains two String fields each) every frame.
        let printer_names: Vec<String> = state.printer_list
            .iter()
            .map(|p| p.display_name.clone())
            .collect();
        egui::ComboBox::from_label("")
            .selected_text(
                state
                    .selected_printer
                    .as_deref()
                    .unwrap_or("Selecione uma impressora"),
            )
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for name in &printer_names {
                    ui.selectable_value(
                        &mut state.selected_printer,
                        Some(name.clone()),
                        name.as_str(),
                    );
                }
            });
    }

    // ── Buttons row (D-04) ────────────────────────────────────────────────────
    ui.add_space(24.0); // lg

    let available_w = ui.available_width();
    let gap = 8.0_f32;
    let test_w = available_w * 0.40 - gap;
    let save_w = available_w * 0.60;

    let can_save = state.agent_token.is_some()
        && state.selected_printer.is_some()
        && !state.is_busy;
    let can_activate = !state.serial_input.trim().is_empty()
        && state.selected_printer.is_some()
        && !state.is_busy;
    let can_test_print = state.selected_printer.is_some() && !state.is_busy;

    let primary_label = if state.agent_token.is_some() {
        "Salvar ativação"
    } else {
        "Ativar"
    };

    // Track if save should be triggered after horizontal layout.
    let mut do_save = false;
    let mut do_activate = false;

    ui.horizontal(|ui| {
        // "Imprimir teste" button (secondary accent)
        if can_test_print {
            if ui
                .add(accent_button("Imprimir teste").min_size(vec2(test_w, 36.0)))
                .clicked()
            {
                handle_test_print(state);
            }
        } else {
            ui.add(
                egui::Button::new(
                    RichText::new("Imprimir teste")
                        .text_style(TextStyle::Button)
                        .color(Color32::from_rgb(97, 97, 97)),
                )
                .fill(Color32::from_rgb(30, 58, 110))
                .min_size(vec2(test_w, 36.0)),
            );
        }

        ui.add_space(gap);

        // Primary "Ativar" / "Salvar ativação" button
        if state.is_busy {
            // Spinner state (D-04) — button remains accent-filled
            ui.add(
                egui::Button::new(
                    RichText::new("Aguarde…")
                        .text_style(TextStyle::Button)
                        .color(Color32::WHITE),
                )
                .fill(Color32::from_rgb(37, 99, 235))
                .min_size(vec2(save_w, 36.0)),
            );
        } else if can_save || can_activate {
            if ui
                .add(accent_button(primary_label).min_size(vec2(save_w, 36.0)))
                .clicked()
                || enter_in_serial
            {
                if state.agent_token.is_some() {
                    do_save = true;
                } else {
                    do_activate = true;
                }
            }
        } else {
            ui.add(
                egui::Button::new(
                    RichText::new(primary_label)
                        .text_style(TextStyle::Button)
                        .color(Color32::from_rgb(97, 97, 97)),
                )
                .fill(Color32::from_rgb(30, 58, 110))
                .min_size(vec2(save_w, 36.0)),
            );
        }
    });

    // Handle actions outside the horizontal closure (avoids double-borrow).
    if do_save {
        handle_save(state, app_dir, conn, should_exit);
    } else if do_activate && can_activate {
        dispatch_activate(state, rt, http, false);
    }

    // ── Test-print status line (D-08, D-09) ──────────────────────────────────
    ui.add_space(4.0);

    match &state.flow {
        FlowState::AwaitingTestConfirm => {
            let mut confirmed_true = false;
            let mut confirmed_false = false;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("A impressão funcionou? ").text_style(TextStyle::Body),
                );
                if ui.button("Sim").clicked() {
                    confirmed_true = true;
                }
                if ui.button("Não").clicked() {
                    confirmed_false = true;
                }
            });
            if confirmed_true {
                state.test_print_confirmed = Some(true);
                state.flow = FlowState::ReadyToSave;
            } else if confirmed_false {
                state.test_print_confirmed = Some(false);
                state.flow = FlowState::ValidatedAwaitingTestPrint;
            }
        }
        FlowState::ReadyToSave => {
            if state.test_print_confirmed == Some(true) {
                ui.label(
                    RichText::new("Impressora pronta.")
                        .text_style(TextStyle::Small)
                        .color(Color32::from_rgb(34, 197, 94)), // #22C55E success green
                );
            } else {
                ui.label(
                    RichText::new(
                        "Tente verificar o papel e o cabo, depois imprima de novo.",
                    )
                    .text_style(TextStyle::Small)
                    .color(Color32::from_rgb(163, 163, 163)),
                );
            }
        }
        _ => {
            if state.test_print_failed {
                ui.label(
                    RichText::new(
                        "Não consegui imprimir — confira papel/cabo. Você ainda pode salvar e ativar.",
                    )
                    .text_style(TextStyle::Small)
                    .color(Color32::from_rgb(239, 68, 68)),
                );
            }
        }
    }

    // ── Autostart warn (D-13) ─────────────────────────────────────────────────
    if let Some(warn) = &state.autostart_warn.clone() {
        ui.add_space(4.0);
        ui.label(
            RichText::new(warn)
                .text_style(TextStyle::Small)
                .color(Color32::from_rgb(163, 163, 163)),
        );
    }
}

// ── Accent button helper (02-UI-SPEC.md Buttons Row) ─────────────────────────

fn accent_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(
        RichText::new(label)
            .text_style(TextStyle::Button)
            .color(Color32::WHITE),
    )
    .fill(Color32::from_rgb(37, 99, 235)) // #2563EB accent
}

// ── Async activate dispatch (Pattern 2) ──────────────────────────────────────

fn dispatch_activate(
    state: &mut ActivationFormState,
    rt: &tokio::runtime::Handle,
    http: &reqwest::Client,
    force_rebind: bool,
) {
    if state.serial_input.trim().is_empty() {
        state.serial_error = Some("Informe o serial antes de ativar.".into());
        return;
    }
    if state.selected_printer.is_none() {
        state.serial_error = Some("Selecione uma impressora antes de salvar.".into());
        return;
    }

    state.is_busy = true;
    state.serial_error = None;
    state.show_rebind_confirm = false;
    state.flow = FlowState::ActivationPending;

    let (tx, rx) = tokio::sync::oneshot::channel();
    state.activate_rx = Some(rx);

    let serial = state.serial_input.trim().to_string();
    let machine_id = machine_id::get_machine_id();
    let base_url = noren_base_url();
    let http = http.clone();

    rt.spawn(async move {
        // CR-03: pass force_rebind so the server can distinguish a confirmed migration
        // from the original activation request that produced the 409 conflict.
        let result = noren_client::activate(
            &http,
            &base_url,
            &serial,
            machine_id.as_deref(),
            force_rebind,
        )
        .await;
        let _ = tx.send(result);
    });
}

// ── Test-print (ACT-05, D-08) ────────────────────────────────────────────────

fn handle_test_print(state: &mut ActivationFormState) {
    let selected = match &state.selected_printer {
        Some(s) => s.clone(),
        None => return,
    };

    let entry = state.printer_list.iter().find(|p| p.display_name == selected);
    let Some(entry) = entry else {
        state.test_print_failed = true;
        return;
    };

    let coupon = build_test_coupon();
    let printer = printer_from_entry(&entry.id);
    match printer.print_raw(&coupon) {
        Ok(()) => {
            state.flow = FlowState::AwaitingTestConfirm;
            state.test_print_failed = false;
        }
        Err(e) => {
            eprintln!("[brevly-print] Test-print error: {e:#}");
            state.test_print_failed = true;
            // Warn-but-allow: flow stays at ValidatedAwaitingTestPrint so Save is still enabled.
        }
    }
}

/// Build the test coupon ESC/POS bytes.
///
/// ESC @ (initialize) + ASCII-safe text + GS V 0 (full cut).
/// No accented characters — thermal printers may not be in UTF-8 mode.
fn build_test_coupon() -> Vec<u8> {
    // Minimal UTC date/time using std (no chrono dep per plan instruction).
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (day, month, year, hour, minute) = unix_secs_to_date(secs);
    let date_str = format!("{day:02}/{month:02}/{year} {hour:02}:{minute:02}");

    let text = format!("Brevly Print - ativacao OK\n{date_str}\n\n\n");

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\x1b\x40"); // ESC @ — initialize printer
    bytes.extend_from_slice(text.as_bytes());
    bytes.extend_from_slice(b"\x1d\x56\x00"); // GS V 0 — full cut
    bytes
}

/// Minimal UTC date/time from Unix seconds (no chrono dependency).
fn unix_secs_to_date(secs: u64) -> (u32, u32, i32, u32, u32) {
    let minute = (secs / 60 % 60) as u32;
    let hour = (secs / 3600 % 24) as u32;
    let days = secs / 86400;

    let mut year = 1970i32;
    let mut remaining = days as i32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let months = [
        31i32,
        if is_leap(year) { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month = 1u32;
    for &days_in_month in &months {
        if remaining < days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }
    let day = remaining as u32 + 1;
    (day, month, year, hour, minute)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ── Save flow (ACT-06, ACT-08, D-13, D-15, Pitfall 8) ───────────────────────

/// Execute the full save flow synchronously (Pitfall 8: ALL persistence BEFORE exit).
///
/// Order: DPAPI → SQLite → autostart → exit 0 (D-15).
fn handle_save(
    state: &mut ActivationFormState,
    app_dir: &std::path::Path,
    conn: &rusqlite::Connection,
    should_exit: &mut bool,
) {
    let agent_token = match state.agent_token.clone() {
        Some(t) => t,
        None => {
            state.serial_error = Some("Token não encontrado. Ative o serial primeiro.".into());
            return;
        }
    };
    let selected_printer = match state.selected_printer.clone() {
        Some(p) => p,
        None => {
            state.serial_error = Some("Selecione uma impressora antes de salvar.".into());
            return;
        }
    };

    state.flow = FlowState::Saving;
    state.is_busy = true;

    // 1. Persist agentToken via DPAPI (ACT-06, T-02-08: never in SQLite, never logged)
    let cred = credential_store(app_dir);
    if let Err(e) = cred.save(agent_token.as_bytes()) {
        eprintln!("[brevly-print] DPAPI save error: {e:#}");
        state.serial_error = Some(format!("Falha ao salvar credencial: {e}"));
        state.is_busy = false;
        state.flow = FlowState::ValidatedAwaitingTestPrint;
        return;
    }

    // 2. Persist printer + tenant config to SQLite (D-15)
    let printer_type = state
        .printer_list
        .iter()
        .find(|p| p.display_name == selected_printer)
        .map(|p| match &p.id {
            PrinterId::Spooler(_) => "spooler",
            PrinterId::Serial(_)  => "serial",
        })
        .unwrap_or("spooler");

    let printer_id_str = state
        .printer_list
        .iter()
        .find(|p| p.display_name == selected_printer)
        .map(|p| match &p.id {
            PrinterId::Spooler(name) => name.clone(),
            PrinterId::Serial(port)  => port.clone(),
        })
        .unwrap_or_else(|| selected_printer.clone());

    let enabled_types_json = serde_json::to_string(&state.enabled_types)
        .unwrap_or_else(|_| "[]".into());
    let tenant_id = state.tenant_id.clone().unwrap_or_default();
    let base_url = noren_base_url();

    let config_entries: Vec<(&str, String)> = vec![
        ("printer_name",  printer_id_str),
        ("printer_type",  printer_type.to_string()),
        ("tenant_id",     tenant_id),
        ("enabled_types", enabled_types_json),
        ("noren_base_url", base_url),
    ];
    for (key, value) in &config_entries {
        if let Err(e) = config_store::set(conn, key, value) {
            eprintln!("[brevly-print] config_store::set({key}) error: {e:#}");
            state.serial_error = Some(format!("Falha ao salvar configuração: {e}"));
            state.is_busy = false;
            state.flow = FlowState::ValidatedAwaitingTestPrint;
            return;
        }
    }

    // 3. Register HKCU autostart (ACT-08, D-13 — warn-not-block on failure)
    register_autostart_warn_on_fail(state);

    // 4. Signal event loop to exit cleanly (D-15, Pitfall 8: all persistence is DONE above).
    // Set should_exit = true and return; main.rs window_event() checks should_exit() on the
    // next RedrawRequested and calls event_loop.exit(), allowing all Drop destructors to run
    // (including rusqlite::Connection::drop → sqlite3_close → WAL flush). CR-01: do NOT call
    // std::process::exit() here — that would bypass Drop and risk SQLite WAL corruption.
    println!("[brevly-print] Activation saved — signalling event loop exit.");
    *should_exit = true;
}

/// Register HKCU Run autostart (ACT-08, Pitfall 4: MUST use CurrentUser not Dynamic).
///
/// D-13: warn on failure, but DO NOT block the save flow.
fn register_autostart_warn_on_fail(state: &mut ActivationFormState) {
    #[cfg(windows)]
    {
        match std::env::current_exe() {
            Ok(exe) => {
                use auto_launch::{AutoLaunch, WindowsEnableMode};
                let al = AutoLaunch::new(
                    "BrevlyPrint",
                    &exe.to_string_lossy(),
                    WindowsEnableMode::CurrentUser, // Pitfall 4: CurrentUser, never Dynamic
                    &[] as &[&str],
                );
                if let Err(e) = al.enable() {
                    eprintln!("[brevly-print] Autostart registration failed: {e:#}");
                    state.autostart_warn = Some(
                        "Não foi possível registrar a inicialização automática. Você pode fazer isso manualmente depois.".into(),
                    );
                }
            }
            Err(e) => {
                eprintln!("[brevly-print] current_exe() error: {e:#}");
                state.autostart_warn = Some(
                    "Não foi possível registrar a inicialização automática. Você pode fazer isso manualmente depois.".into(),
                );
            }
        }
    }
    // On Linux (dev): autostart is a no-op — no warning needed (#[cfg(windows)] gate above).
    let _ = state; // suppress unused variable warning on non-Windows
}
