//! Touch-friendly Android file/save/export/import dialog with Storage Access Framework (SAF) and
//! runtime storage permission integration.
//!
//! When [`wordcraft_ui_egui::WordApp`] requests an Open, Import, Insert Picture, Add Font,
//! Load Recipients, Import Sources, Save As, Export, or Print to PDF path via
//! [`wordcraft_ui_egui::Services::file_dialog`], this module opens an interactive modal dialog that
//! supports both:
//! 1. **Direct Android storage browsing** (`Documents`, `Downloads`, `Pictures`, `Templates`,
//!    `Shared Storage`, `App External`, `App Exports`, `App Internal`) when storage permissions are
//!    granted.
//! 2. **Android Storage Access Framework (SAF)** system picker (`ACTION_OPEN_DOCUMENT`,
//!    `ACTION_CREATE_DOCUMENT`, `ACTION_OPEN_DOCUMENT_TREE`) and runtime permission requests via
//!    JNI hooks.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock, PoisonError};

use wordcraft_ui_egui::{FileDialogAnswer, FileDialogRequest, open_filters};

use crate::permissions::{self, AndroidDirs};
use crate::services;

/// Save As / Export formats offered in the Android Save dialog.
pub const SAVE_FORMATS: &[(&str, &str)] = &[
    ("Word Document (.docx)", "docx"),
    ("Word Macro-Enabled Document (.docm)", "docm"),
    ("Word Template (.dotx)", "dotx"),
    ("Word Macro-Enabled Template (.dotm)", "dotm"),
    ("PDF Document (.pdf)", "pdf"),
    ("OpenDocument Text (.odt)", "odt"),
    ("Rich Text Format (.rtf)", "rtf"),
    ("Web Page (.html)", "html"),
    ("Markdown (.md)", "md"),
    ("Plain Text (.txt)", "txt"),
    ("LaTeX (.tex)", "tex"),
    ("PNG Page Image (.png)", "png"),
    ("Word 2003 XML (.xml)", "xml"),
    ("WordCraft JSON (.json)", "json"),
];

/// Optional platform bridge callbacks registered by `android_entry` when running inside the
/// Android JNI runtime.
pub struct PlatformBridge {
    /// Request runtime storage permissions (`READ_MEDIA_IMAGES` / `READ_EXTERNAL_STORAGE` /
    /// `WRITE_EXTERNAL_STORAGE` / `MANAGE_EXTERNAL_STORAGE`).
    pub request_permissions: Box<dyn Fn(bool) + Send + Sync>,
    /// Launch Android SAF `ACTION_OPEN_DOCUMENT` picker (`multiple`).
    pub launch_saf_open: Box<dyn Fn(bool) + Send + Sync>,
    /// Launch Android SAF `ACTION_CREATE_DOCUMENT` picker (`suggested_name`).
    pub launch_saf_save: Box<dyn Fn(&str) + Send + Sync>,
    /// Launch Android SAF `ACTION_OPEN_DOCUMENT_TREE` folder picker.
    pub launch_saf_folder: Box<dyn Fn() + Send + Sync>,
    /// Notify Android `MediaScannerConnection` so an exported file is indexed by the system.
    pub scan_media_file: Box<dyn Fn(&str) + Send + Sync>,
}

static BRIDGE: OnceLock<Mutex<Option<PlatformBridge>>> = OnceLock::new();

fn bridge_lock() -> &'static Mutex<Option<PlatformBridge>> {
    BRIDGE.get_or_init(|| Mutex::new(None))
}

/// Register the Android JNI bridge callbacks.
pub fn set_platform_bridge(bridge: PlatformBridge) {
    *bridge_lock().lock().unwrap_or_else(PoisonError::into_inner) = Some(bridge);
}

/// Ask Android to request runtime storage permissions (`all_files`: also open the
/// `MANAGE_EXTERNAL_STORAGE` settings page on Android 11+).
pub fn request_storage_permissions(all_files: bool) {
    if let Some(b) = bridge_lock().lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        (b.request_permissions)(all_files);
    }
    permissions::refresh_permission_probe();
}

/// Notify Android MediaScanner that a file was saved or exported so it appears in Android Files /
/// Gallery / Downloads immediately.
pub fn notify_media_scanner(path: &str) {
    if let Some(b) = bridge_lock().lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
        (b.scan_media_file)(path);
    }
}

/// Dialog mode: opening/importing one or more existing files or saving/exporting a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DialogMode {
    Open { purpose: String, multiple: bool },
    Save { suggested: String, dir: Option<String> },
}

struct ActiveDialog {
    mode: DialogMode,
    current_dir: PathBuf,
    entries: Vec<DirEntryItem>,
    selected_path: Option<PathBuf>,
    file_name_input: String,
    selected_format_idx: usize,
    new_folder_name: String,
    show_new_folder: bool,
    error_message: Option<String>,
    reply_tx: Option<Sender<FileDialogAnswer>>,
}

#[derive(Clone, Debug)]
struct DirEntryItem {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
}

static ACTIVE_DIALOG: OnceLock<Mutex<Option<ActiveDialog>>> = OnceLock::new();
static PENDING_MEDIA_SCANS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
static PENDING_STATUS_NOTES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn dialog_lock() -> &'static Mutex<Option<ActiveDialog>> {
    ACTIVE_DIALOG.get_or_init(|| Mutex::new(None))
}

fn scans_lock() -> &'static Mutex<Vec<String>> {
    PENDING_MEDIA_SCANS.get_or_init(|| Mutex::new(Vec::new()))
}

fn notes_lock() -> &'static Mutex<Vec<String>> {
    PENDING_STATUS_NOTES.get_or_init(|| Mutex::new(Vec::new()))
}

/// Drain saved/exported file paths that should be indexed by Android's `MediaScannerConnection`
/// (and flushed to any pending SAF `content://` output stream) after `WordApp::logic` writes them.
pub fn take_pending_media_scans() -> Vec<String> {
    std::mem::take(&mut *scans_lock().lock().unwrap_or_else(PoisonError::into_inner))
}

/// Drain any scoped-storage fallback notices generated when resolving an export destination.
pub fn take_pending_status_notes() -> Vec<String> {
    std::mem::take(&mut *notes_lock().lock().unwrap_or_else(PoisonError::into_inner))
}

fn queue_media_scan(path: String) {
    scans_lock().lock().unwrap_or_else(PoisonError::into_inner).push(path);
}

fn queue_status_note(note: String) {
    notes_lock().lock().unwrap_or_else(PoisonError::into_inner).push(note);
}

/// Open the Android File Browser modal for a [`FileDialogRequest`] from [`wordcraft_ui_egui::WordApp`]
/// and return the channel receiver that delivers the user's selection.
pub fn start_request(req: FileDialogRequest) -> Receiver<FileDialogAnswer> {
    let (tx, rx) = channel();
    let dirs = permissions::current_dirs();
    permissions::refresh_permission_probe();

    let mut active = match req {
        FileDialogRequest::Open { purpose } => {
            let current_dir = dirs.default_open_dir(&purpose);
            ActiveDialog {
                mode: DialogMode::Open {
                    purpose,
                    multiple: false,
                },
                current_dir,
                entries: Vec::new(),
                selected_path: None,
                file_name_input: String::new(),
                selected_format_idx: 0,
                new_folder_name: String::new(),
                show_new_folder: false,
                error_message: None,
                reply_tx: Some(tx),
            }
        }
        FileDialogRequest::OpenMany { purpose } => {
            let current_dir = dirs.default_open_dir(&purpose);
            ActiveDialog {
                mode: DialogMode::Open {
                    purpose,
                    multiple: true,
                },
                current_dir,
                entries: Vec::new(),
                selected_path: None,
                file_name_input: String::new(),
                selected_format_idx: 0,
                new_folder_name: String::new(),
                show_new_folder: false,
                error_message: None,
                reply_tx: Some(tx),
            }
        }
        FileDialogRequest::Save { name, dir } => {
            let path = Path::new(&name);
            let current_dir = dir
                .as_deref()
                .map(PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty() && (p.is_dir() || std::fs::create_dir_all(p).is_ok()))
                .or_else(|| {
                    path.parent()
                        .filter(|p| !p.as_os_str().is_empty() && p.is_dir() && permissions::is_dir_writable(p))
                        .map(Path::to_path_buf)
                })
                .unwrap_or_else(|| dirs.default_save_dir());
            let file_name_input = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Document1.docx".into());
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase)
                .unwrap_or_else(|| "docx".into());
            let selected_format_idx = SAVE_FORMATS.iter().position(|(_, e)| *e == ext).unwrap_or(0);
            ActiveDialog {
                mode: DialogMode::Save { suggested: name, dir },
                current_dir,
                entries: Vec::new(),
                selected_path: None,
                file_name_input,
                selected_format_idx,
                new_folder_name: String::new(),
                show_new_folder: false,
                error_message: None,
                reply_tx: Some(tx),
            }
        }
    };

    refresh_entries(&mut active);
    let mut guard = dialog_lock().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(mut old) = guard.take()
        && let Some(old_tx) = old.reply_tx.take()
    {
        let _ = old_tx.send(Ok(Vec::new()));
    }
    *guard = Some(active);
    rx
}

/// Complete the active open dialog with file contents returned from the Android SAF picker.
pub fn deliver_saf_open_contents(files: Vec<(String, Vec<u8>)>) {
    let Some(mut active) = dialog_lock().lock().unwrap_or_else(PoisonError::into_inner).take() else {
        for (name, bytes) in files {
            if !name.is_empty() {
                services::enqueue_incoming_file(name, bytes);
            }
        }
        return;
    };
    let Some(tx) = active.reply_tx.take() else {
        return;
    };
    let mut staged_paths = Vec::new();
    for (name, bytes) in files {
        if !name.is_empty() {
            match services::stage_saf_bytes(&name, &bytes) {
                Ok(staged) => staged_paths.push(staged.to_string_lossy().into_owned()),
                Err(e) => {
                    queue_status_note(format!("Could not stage SAF file {name}: {e}"));
                }
            }
        }
    }
    let _ = tx.send(Ok(staged_paths));
}

/// Complete the active save dialog with a path returned from the Android SAF picker.
pub fn deliver_saf_save_path(path: String) {
    let Some(mut active) = dialog_lock().lock().unwrap_or_else(PoisonError::into_inner).take() else {
        return;
    };
    let Some(tx) = active.reply_tx.take() else {
        return;
    };
    if path.trim().is_empty() {
        let _ = tx.send(Ok(Vec::new()));
        return;
    }
    let dirs = permissions::current_dirs();
    let (resolved, fallback_notice) = permissions::resolve_export_target(Path::new(&path), &dirs);
    let resolved_str = resolved.to_string_lossy().into_owned();
    queue_media_scan(resolved_str.clone());
    if let Some(note) = fallback_notice {
        queue_status_note(note);
    }
    let _ = tx.send(Ok(vec![resolved_str]));
}

/// Update the active dialog's directory when the user picks a folder via Android SAF
/// `ACTION_OPEN_DOCUMENT_TREE`.
pub fn deliver_saf_folder_path(path: String) {
    if path.trim().is_empty() {
        return;
    }
    let mut guard = dialog_lock().lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(active) = guard.as_mut() {
        active.current_dir = PathBuf::from(path);
        active.selected_path = None;
        refresh_entries(active);
    }
}

/// Apply the selected format extension to `name` when saving.
pub fn apply_save_extension(name: &str, ext: &str) -> String {
    let trimmed = name.trim();
    let base = if trimmed.is_empty() { "Document1" } else { trimmed };
    let p = Path::new(base);
    match p.extension().and_then(|e| e.to_str()) {
        Some(existing) if existing.eq_ignore_ascii_case(ext) => base.to_string(),
        Some(_) => p.with_extension(ext).to_string_lossy().into_owned(),
        None => format!("{base}.{ext}"),
    }
}

fn allowed_extensions_for_mode(mode: &DialogMode) -> Vec<&'static str> {
    match mode {
        DialogMode::Open { purpose, .. } => {
            let filters = open_filters(purpose);
            filters
                .first()
                .map(|(_, exts)| exts.to_vec())
                .unwrap_or_else(|| vec!["*"])
        }
        DialogMode::Save { .. } => SAVE_FORMATS.iter().map(|(_, ext)| *ext).collect(),
    }
}

fn matches_extension_filter(path: &Path, allowed: &[&str]) -> bool {
    if allowed.contains(&"*") {
        return true;
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let lower = ext.to_ascii_lowercase();
    allowed.iter().any(|a| *a == lower)
}

fn refresh_entries(active: &mut ActiveDialog) {
    active.entries.clear();
    active.error_message = None;
    let rd = match std::fs::read_dir(&active.current_dir) {
        Ok(rd) => rd,
        Err(e) => {
            active.error_message = Some(format!(
                "Cannot read {}: {e}. Tap 'Grant Storage Access' or 'System File Picker (SAF)' above, or select an App Storage bookmark.",
                active.current_dir.display()
            ));
            return;
        }
    };
    let allowed = allowed_extensions_for_mode(&active.mode);
    let is_open = matches!(active.mode, DialogMode::Open { .. });
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir) || path.is_dir();
        if !is_dir && is_open && !matches_extension_filter(&path, &allowed) {
            continue;
        }
        let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
        active.entries.push(DirEntryItem {
            name,
            path,
            is_dir,
            size,
        });
    }
    active.entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
}

fn dialog_title(mode: &DialogMode) -> &'static str {
    match mode {
        DialogMode::Open { purpose, .. } => match purpose.as_str() {
            "picture" => "Insert / Change Picture (Android)",
            "font" => "Add Font Files (Android)",
            "recipients" => "Select Mail Merge Recipients (Android)",
            "sources" => "Import Bibliography Sources (.xml) (Android)",
            _ => "Open Document (Android)",
        },
        DialogMode::Save { .. } => "Save / Export Document (Android)",
    }
}

/// Render the touch-friendly Android File Dialog modal if one is currently open.
pub fn render_modal(ctx: &egui::Context) {
    let mut guard = dialog_lock().lock().unwrap_or_else(PoisonError::into_inner);
    let Some(active) = guard.as_mut() else {
        return;
    };

    let dirs: AndroidDirs = permissions::current_dirs();
    let perm_status = permissions::current_status();
    let title = dialog_title(&active.mode);

    let mut close_dialog = false;
    let mut answer: Option<FileDialogAnswer> = None;

    egui::Window::new(title)
        .id(egui::Id::unique("wordcraft_android_file_dialog_modal"))
        .collapsible(false)
        .resizable(true)
        .default_size([720.0, 480.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            // Top bar: Storage Permission status + Grant Permission & SAF System Picker buttons
            ui.horizontal_wrapped(|ui| {
                ui.label(perm_status.summary());
                if ui.button("Grant Storage Access").clicked() {
                    request_storage_permissions(false);
                    refresh_entries(active);
                }
                if perm_status.sdk_int >= 30
                    && !perm_status.all_files_access
                    && ui.button("All Files Access").clicked()
                {
                    request_storage_permissions(true);
                    refresh_entries(active);
                }
                let has_bridge = bridge_lock().lock().unwrap_or_else(PoisonError::into_inner).is_some();
                if has_bridge && ui.button("System File Picker (SAF)").clicked() {
                    if let Some(b) = bridge_lock().lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                        match &active.mode {
                            DialogMode::Open { multiple, .. } => (b.launch_saf_open)(*multiple),
                            DialogMode::Save { .. } => (b.launch_saf_save)(&active.file_name_input),
                        }
                    }
                }
                if has_bridge && ui.button("Pick Folder (SAF)").clicked() {
                    if let Some(b) = bridge_lock().lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
                        (b.launch_saf_folder)();
                    }
                }
            });

            ui.separator();

            // Quick bookmarks bar
            ui.horizontal_wrapped(|ui| {
                ui.strong("Locations:");
                for (label, target_dir) in dirs.bookmarks() {
                    let selected = active.current_dir == target_dir;
                    if ui.selectable_label(selected, label).clicked() {
                        let _ = std::fs::create_dir_all(&target_dir);
                        active.current_dir = target_dir;
                        active.selected_path = None;
                        refresh_entries(active);
                    }
                }
                if let Some(parent) = active.current_dir.parent().map(Path::to_path_buf)
                    && ui.button("⬆ Up").clicked()
                {
                    active.current_dir = parent;
                    active.selected_path = None;
                    refresh_entries(active);
                }
                if ui.button("⟳ Refresh").clicked() {
                    permissions::refresh_permission_probe();
                    refresh_entries(active);
                }
                if ui.button("+ New Folder").clicked() {
                    active.show_new_folder = !active.show_new_folder;
                }
            });

            if active.show_new_folder {
                ui.horizontal(|ui| {
                    ui.label("Folder name:");
                    ui.text_edit_singleline(&mut active.new_folder_name);
                    if ui.button("Create").clicked() && !active.new_folder_name.trim().is_empty() {
                        let new_dir = active.current_dir.join(active.new_folder_name.trim());
                        match std::fs::create_dir_all(&new_dir) {
                            Ok(()) => {
                                active.new_folder_name.clear();
                                active.show_new_folder = false;
                                active.current_dir = new_dir;
                                refresh_entries(active);
                            }
                            Err(e) => active.error_message = Some(format!("Could not create folder: {e}")),
                        }
                    }
                });
            }

            ui.label(format!("Current folder: {}", active.current_dir.display()));

            if let Some(err) = &active.error_message {
                ui.colored_label(egui::Color32::from_rgb(240, 140, 100), err);
            }

            ui.separator();

            // File list
            let mut navigate_to: Option<PathBuf> = None;
            let mut double_clicked_file: Option<PathBuf> = None;

            egui::ScrollArea::vertical()
                .max_height(250.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if active.entries.is_empty() && active.error_message.is_none() {
                        ui.label("No matching files in this folder.");
                    }
                    for entry in &active.entries {
                        let is_selected = active.selected_path.as_ref() == Some(&entry.path);
                        let label = if entry.is_dir {
                            format!("📁 {}", entry.name)
                        } else {
                            format!("📄 {} ({:.1} KB)", entry.name, entry.size as f64 / 1024.0)
                        };
                        let resp = ui.selectable_label(is_selected, label);
                        if resp.clicked() {
                            if entry.is_dir {
                                navigate_to = Some(entry.path.clone());
                            } else {
                                if matches!(active.mode, DialogMode::Save { .. }) {
                                    active.file_name_input = entry.name.clone();
                                }
                                active.selected_path = Some(entry.path.clone());
                            }
                        }
                        if resp.double_clicked() && !entry.is_dir {
                            double_clicked_file = Some(entry.path.clone());
                        }
                    }
                });

            if let Some(dir) = navigate_to {
                active.current_dir = dir;
                active.selected_path = None;
                refresh_entries(active);
            }
            if let Some(file) = double_clicked_file
                && matches!(active.mode, DialogMode::Open { .. })
            {
                answer = Some(Ok(vec![file.to_string_lossy().into_owned()]));
                close_dialog = true;
            }

            ui.separator();

            // Save filename & format selector
            if matches!(active.mode, DialogMode::Save { .. }) {
                ui.horizontal(|ui| {
                    ui.label("File name:");
                    ui.text_edit_singleline(&mut active.file_name_input);
                });
                ui.horizontal(|ui| {
                    ui.label("Format:");
                    let current_fmt = SAVE_FORMATS
                        .get(active.selected_format_idx)
                        .copied()
                        .unwrap_or(SAVE_FORMATS[0]);
                    egui::ComboBox::from_id_salt("wordcraft_android_save_format_combo")
                        .selected_text(current_fmt.0)
                        .show_ui(ui, |ui| {
                            for (idx, (label, ext)) in SAVE_FORMATS.iter().enumerate() {
                                if ui
                                    .selectable_value(&mut active.selected_format_idx, idx, *label)
                                    .clicked()
                                {
                                    active.file_name_input =
                                        apply_save_extension(&active.file_name_input, ext);
                                }
                            }
                        });
                });
            }

            ui.separator();

            // Bottom action buttons
            ui.horizontal(|ui| {
                match &active.mode {
                    DialogMode::Open { purpose, multiple } => {
                        let can_open = active.selected_path.is_some();
                        let btn_label = match purpose.as_str() {
                            "picture" => "Insert Picture",
                            "font" => "Add Selected Font",
                            "recipients" => "Use Recipient List",
                            "sources" => "Import Sources",
                            _ => "Open Document",
                        };
                        if ui.add_enabled(can_open, egui::Button::new(btn_label)).clicked()
                            && let Some(p) = &active.selected_path
                        {
                            answer = Some(Ok(vec![p.to_string_lossy().into_owned()]));
                            close_dialog = true;
                        }
                        if *multiple {
                            let all_files: Vec<String> = active
                                .entries
                                .iter()
                                .filter(|e| !e.is_dir)
                                .map(|e| e.path.to_string_lossy().into_owned())
                                .collect();
                            if !all_files.is_empty() && ui.button("Add All in Folder").clicked() {
                                answer = Some(Ok(all_files));
                                close_dialog = true;
                            }
                        }
                    }
                    DialogMode::Save { .. } => {
                        let (_, ext) = SAVE_FORMATS
                            .get(active.selected_format_idx)
                            .copied()
                            .unwrap_or(SAVE_FORMATS[0]);
                        let final_name = apply_save_extension(&active.file_name_input, ext);
                        if ui.button("Save / Export Here").clicked() {
                            let requested = active.current_dir.join(final_name);
                            let (resolved, fallback_notice) =
                                permissions::resolve_export_target(&requested, &dirs);
                            let resolved_str = resolved.to_string_lossy().into_owned();
                            queue_media_scan(resolved_str.clone());
                            if let Some(note) = fallback_notice {
                                queue_status_note(note);
                            }
                            answer = Some(Ok(vec![resolved_str]));
                            close_dialog = true;
                        }
                        if bridge_lock().lock().unwrap_or_else(PoisonError::into_inner).is_some()
                            && ui.button("Save With System Picker").clicked()
                            && let Some(b) =
                                bridge_lock().lock().unwrap_or_else(PoisonError::into_inner).as_ref()
                        {
                            let (_, ext) = SAVE_FORMATS
                                .get(active.selected_format_idx)
                                .copied()
                                .unwrap_or(SAVE_FORMATS[0]);
                            let final_name = apply_save_extension(&active.file_name_input, ext);
                            (b.launch_saf_save)(&final_name);
                        }
                    }
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(Ok(Vec::new()));
                    close_dialog = true;
                }
            });
        });

    if let Some(ans) = answer
        && let Some(tx) = active.reply_tx.take()
    {
        let _ = tx.send(ans);
    }
    if close_dialog {
        *guard = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_extension_is_applied_cleanly() {
        assert_eq!(apply_save_extension("Report", "docx"), "Report.docx");
        assert_eq!(apply_save_extension("Report.docx", "pdf"), "Report.pdf");
        assert_eq!(apply_save_extension("Report.PDF", "pdf"), "Report.PDF");
        assert_eq!(apply_save_extension("", "odt"), "Document1.odt");
    }

    #[test]
    fn saf_save_path_resolves_and_answers_channel() {
        let _ = take_pending_media_scans();
        let _ = take_pending_status_notes();
        let tmp = std::env::temp_dir().join(format!("wordcraft-saf-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        permissions::init_dirs(Some(tmp.clone()), Some(tmp.clone()));

        let rx = start_request(FileDialogRequest::Save {
            name: "Proposal.pdf".into(),
            dir: Some(tmp.to_string_lossy().into_owned()),
        });
        let target = tmp.join("Proposal.pdf");
        deliver_saf_save_path(target.to_string_lossy().into_owned());

        let ans = rx.try_recv();
        assert_eq!(ans, Ok(Ok(vec![target.to_string_lossy().into_owned()])));
        assert_eq!(
            take_pending_media_scans(),
            vec![target.to_string_lossy().into_owned()]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
