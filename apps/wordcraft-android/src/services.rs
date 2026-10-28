//! Android platform services: document/picture/font/recipient/source import & export,
//! permission-aware file writing, SAF staging, preferences (`ui.json`), custom dictionary
//! (`custom-dictionary.txt`), user fonts (`fonts/`), templates (`Templates/`), and crash recovery
//! (`recovery/`).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use wordcraft_ui_egui::{Services, UiState};

use crate::file_dialog;
use crate::permissions;

/// Thread-safe queue of `(file_name, file_bytes)` delivered from Android `ACTION_VIEW` /
/// `ACTION_EDIT` / `ACTION_SEND` intents.
pub type Inbox = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

static GLOBAL_INBOX: OnceLock<Inbox> = OnceLock::new();

pub fn global_inbox() -> Inbox {
    GLOBAL_INBOX.get_or_init(|| Arc::new(Mutex::new(Vec::new()))).clone()
}

/// Enqueue an incoming document, picture, font, or recipient list from an Android Intent
/// (`MainActivity`).
pub fn enqueue_incoming_file(name: String, bytes: Vec<u8>) {
    global_inbox()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((name, bytes));
}

/// Per-user profile/configuration directory on Android (`internal_data` from `AndroidApp`, or
/// `WORDCRAFT_PROFILE_DIR`).
pub fn config_dir() -> PathBuf {
    permissions::current_dirs().internal_data
}

pub fn prefs_file() -> PathBuf {
    config_dir().join("ui.json")
}

pub fn dictionary_file() -> PathBuf {
    config_dir().join("custom-dictionary.txt")
}

pub fn templates_dir() -> PathBuf {
    permissions::current_dirs().templates_dir()
}

pub fn fonts_dir() -> PathBuf {
    permissions::current_dirs().fonts_dir()
}

/// Initialize Android support directories (`Templates/`, `fonts/`, `recovery/`) and load any
/// user-installed fonts and custom dictionary entries.
pub fn init_support_dirs() {
    let cfg = config_dir();
    let _ = std::fs::create_dir_all(&cfg);
    let _ = std::fs::create_dir_all(templates_dir());
    let _ = std::fs::create_dir_all(fonts_dir());
    let _ = std::fs::create_dir_all(cfg.join("recovery"));
    let _ = wordcraft_fonts::FontDb::global().load_user_fonts(&fonts_dir());
    wordcraft_proof::load_user_dictionary(&dictionary_file());
}

/// Read persisted [`UiState`] preferences from `ui.json`, if present.
pub fn load_ui_prefs() -> Option<UiState> {
    let bytes = std::fs::read(prefs_file()).ok()?;
    UiState::from_json(&bytes)
}

/// Stage file bytes returned from Android SAF (`content://` URI) into the app's writable `Imports`
/// directory so path-based WordCraft commands (`file.open`, `insert.picture`, `font.add`,
/// `references.sources`, `mailings.recipients`) can read the file by path.
pub fn stage_saf_bytes(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let dirs = permissions::current_dirs();
    let stage_dir = dirs.imports_staging_dir();
    std::fs::create_dir_all(&stage_dir)
        .map_err(|e| format!("create staging dir {}: {e}", stage_dir.display()))?;
    let safe_name: String = Path::new(name)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "imported.docx".into());
    let dest = stage_dir.join(safe_name);
    std::fs::write(&dest, bytes)
        .map_err(|e| format!("write staged file {}: {e}", dest.display()))?;
    Ok(dest)
}

/// Write `bytes` to `path` with Android scoped-storage fallback and `MediaScannerConnection`
/// indexing.
pub fn write_with_android_permissions(
    path: &Path,
    bytes: &[u8],
) -> Result<(PathBuf, Option<String>), String> {
    let dirs = permissions::current_dirs();
    let (target, fallback_note) = permissions::resolve_export_target(path, &dirs);
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
    if let Some(target_str) = target.to_str() {
        file_dialog::notify_media_scanner(target_str);
    }
    Ok((target, fallback_note))
}

/// Construct the Android [`Services`] table for [`wordcraft_ui_egui::WordApp`].
pub fn android_services() -> Services {
    Services {
        file_dialog: Some(Box::new(file_dialog::start_request)),
        inbox: Some(global_inbox()),
        templates_dir: Some(templates_dir()),
        animations_enabled: Some(Box::new(|| Some(true))),
        ..Services::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wordcraft_doc::{Document, StoryRef};
    use wordcraft_engine::Session;

    #[test]
    fn android_services_roundtrip_docx_odt_rtf_html_md_txt_pdf_png_with_permissions() {
        let dir = std::env::temp_dir().join(format!("wordcraft-android-svc-test-{}", std::process::id()));
        let internal = dir.join("internal");
        let external = dir.join("external");
        let shared = dir.join("shared");
        let documents = shared.join("Documents");
        let _ = std::fs::create_dir_all(&documents);
        permissions::init_dirs(Some(internal), Some(external));
        init_support_dirs();

        let mut session = Session::new(Document::new());
        session
            .run("text.insert", &json!({ "text": "Hello from WordCraft on Android!" }))
            .unwrap();

        // 1. Save / Export DOCX, ODT, RTF, HTML, Markdown, Plain Text, LaTeX, PDF, and PNG to Android Documents.
        for ext in ["docx", "odt", "rtf", "html", "md", "txt", "tex", "pdf"] {
            let out_path = documents.join(format!("android_doc.{ext}"));
            let (resolved, note) =
                permissions::resolve_export_target(&out_path, &permissions::current_dirs());
            assert_eq!(resolved, out_path);
            assert!(note.is_none());

            session
                .run("file.saveAs", &json!({ "path": resolved.to_string_lossy() }))
                .unwrap();
            assert!(
                resolved.is_file(),
                "expected exported {ext} file at {}",
                resolved.display()
            );
            let metadata = std::fs::metadata(&resolved).unwrap();
            assert!(metadata.len() > 0, "exported {ext} file should be non-empty");
        }

        let png_path = documents.join("android_doc.png");
        session
            .run("file.exportPng", &json!({ "path": png_path.to_string_lossy() }))
            .unwrap();
        assert!(png_path.is_file());

        // 2. Re-open exported DOCX, ODT, RTF, Markdown, and HTML into a fresh Session.
        for ext in ["docx", "odt", "rtf", "md", "html"] {
            let mut verify_session = Session::new(Document::new());
            verify_session
                .run(
                    "file.open",
                    &json!({ "path": documents.join(format!("android_doc.{ext}")).to_string_lossy() }),
                )
                .unwrap();
            let text = verify_session.doc.plain_text(StoryRef::Body);
            assert!(
                text.contains("Hello from WordCraft on Android!"),
                "re-opened {ext} should preserve document text, got: {text:?}"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }
}
