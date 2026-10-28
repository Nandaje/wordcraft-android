//! Android shell wrapping [`wordcraft_ui_egui::WordApp`] with touch-twin filtering, hardware GPU
//! acceleration (Vulkan 1.1+ / OpenGL ES 3.0+), incoming file intent handling, media scanner / SAF
//! synchronization, and the Android file/permission modal overlay.

use std::sync::{Arc, Mutex, PoisonError};

use wordcraft_doc::Document;
use wordcraft_engine::Session;
use wordcraft_ui_egui::WordApp;

use crate::file_dialog;
use crate::gpu_startup::{self, GpuBackend, Plan, RenderingMode, SharedSentinel};
use crate::services;
use crate::touch_twins;

/// The `eframe::App` wrapper for WordCraft on Android.
pub struct AndroidShell {
    pub app: WordApp,
    started_sentinel: SharedSentinel,
    remember_backend: Option<GpuBackend>,
    first_frame_done: bool,
    last_saved_prefs: String,
}

impl AndroidShell {
    /// Construct the Android shell inside `eframe::run_native`'s creation callback.
    pub fn create(
        cc: &eframe::CreationContext<'_>,
        plan: &Plan,
        mode: RenderingMode,
        started_sentinel: SharedSentinel,
        gpu_note: Arc<Mutex<Option<String>>>,
    ) -> Self {
        services::init_support_dirs();

        let mut session = Session::new(Document::new());
        session.user_dict_path = Some(services::dictionary_file());
        let ctx_wake = cc.egui_ctx.clone();
        if let Err(e) = session.enable_recovery(
            services::config_dir().join("recovery"),
            Arc::new(move || ctx_wake.request_repaint()),
        ) {
            log::warn!("wordcraft-android: local recovery unavailable: {e}");
        }

        let svcs = services::android_services();
        let mut app = WordApp::new(session, svcs);
        app.integrated_titlebar = false;

        if let Some(prefs) = services::load_ui_prefs() {
            app.apply_prefs(prefs);
        }

        // Show the Start screen (Blank document, templates, Recent) unless an incoming Intent file
        // is already waiting in the inbox.
        let has_incoming = !services::global_inbox()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty();
        if !has_incoming {
            app.start_screen();
        }

        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            let adapter_info = rs.adapter.get_info();
            log::info!(
                "wordcraft-android: wgpu adapter {:?} ({:?}, backend {:?})",
                adapter_info.name,
                adapter_info.device_type,
                adapter_info.backend
            );
            let software_window = adapter_info.device_type == eframe::wgpu::DeviceType::Cpu;
            if (software_window || plan.backend == GpuBackend::Cpu || mode == RenderingMode::Cpu)
                && let Some(note) = gpu_note.lock().unwrap_or_else(PoisonError::into_inner).clone()
            {
                app.status(note);
            }
        }

        if let Some(reason) = &plan.reason {
            app.status(format!("GPU startup: {reason}"));
        }

        let last_saved_prefs = serde_json::to_string_pretty(&app.prefs()).unwrap_or_default();

        Self {
            app,
            started_sentinel,
            remember_backend: gpu_startup::backend_to_remember(plan),
            first_frame_done: false,
            last_saved_prefs,
        }
    }

    fn flush_post_dialog_side_effects(&mut self) {
        if !self.app.file_dialog_open() {
            for path in file_dialog::take_pending_media_scans() {
                file_dialog::notify_media_scanner(&path);
            }
            for note in file_dialog::take_pending_status_notes() {
                self.app.status(note);
            }
        }
    }

    fn persist_settings_if_changed(&mut self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.app.prefs())
            && json != self.last_saved_prefs
        {
            let _ = std::fs::write(services::prefs_file(), &json);
            self.last_saved_prefs = json;
        }
    }
}

impl eframe::App for AndroidShell {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // Deduplicate simultaneous pointer + touch events on Android touchscreens so a single tap
        // on a ribbon tab or menu item does not open and immediately close the menu.
        touch_twins::drop_twins(ctx, raw_input);
        self.app.raw_input_hook(ctx, raw_input);
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.app.logic(ctx);
        self.flush_post_dialog_side_effects();

        if !self.first_frame_done {
            self.first_frame_done = true;
            if let Some(s) = self
                .started_sentinel
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                s.finish();
            }
            if let Some(b) = self.remember_backend.take() {
                log::info!("wordcraft-android: remembering fallback GPU backend {}", b.name());
            }
        }

        self.persist_settings_if_changed();

        if self.app.quit_requested {
            self.app.quit_requested = false;
            if self.app.close_requested() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        file_dialog::render_modal(ui.ctx());
    }

    fn on_exit(&mut self) {
        self.persist_settings_if_changed();
        self.app.session.finish_recovery();
    }
}
