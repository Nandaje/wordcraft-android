//! WordCraft for Android (`arm64-v8a` / `x86_64`).
//!
//! This crate packages the full WordCraft word processor (`wordcraft-doc`, `wordcraft-fonts`,
//! `wordcraft-proof`, `wordcraft-layout`, `wordcraft-docx`, `wordcraft-formats`,
//! `wordcraft-render`, `wordcraft-pdf`, `wordcraft-engine`, `wordcraft-ui-egui`) as an Android
//! native library (`libwordcraft_android.so`) with:
//!
//! 1. **Hardware GPU Acceleration** ([`gpu_startup`]):
//!    - Configures `eframe` / `wgpu` with **Vulkan 1.1+** (`wgpu::Backends::VULKAN`) and
//!      **OpenGL ES 3.0+** (`wgpu::Backends::GL`) backends using
//!      `wgpu::PowerPreference::HighPerformance`.
//!    - Prefers discrete/integrated mobile GPUs (Qualcomm Adreno, ARM Mali / Immortalis, Samsung
//!      Xclipse, PowerVR) on Vulkan first, falling back to OpenGL ES 3.0+ if a device's Vulkan
//!      driver cannot create a surface or fails during startup.
//!    - Unlocks the adapter's hardware `max_buffer_size` and `max_texture_dimension_2d` limits so
//!      high-DPI page textures and multi-page spreads render smoothly on the GPU.
//!    - Includes crash-sentinel protection (`gpu-starting.json`) that automatically steps down
//!      (`Vulkan` -> `OpenGL ES` -> `CPU`) if a driver crashes during initialization.
//!
//! 2. **File & Media Storage Permissions for Import, Open, Save & Export** ([`permissions`],
//!    [`file_dialog`], [`services`]):
//!    - Supports Android runtime storage permissions across API levels:
//!      - `READ_EXTERNAL_STORAGE` & `WRITE_EXTERNAL_STORAGE` (Android 8–10 / API 26–29)
//!      - `READ_EXTERNAL_STORAGE` & `MANAGE_EXTERNAL_STORAGE` (Android 11–12L / API 30–32)
//!      - `READ_MEDIA_IMAGES` & `MANAGE_EXTERNAL_STORAGE` (Android 13+ / API 33+)
//!    - Provides an in-app touch-friendly **Android File Browser & Exporter** modal integrated with
//!      both shared storage directories (`Downloads`, `Documents`, `Pictures`, `Templates`,
//!      `Fonts`, app-scoped `Exports`/`Imports`) and the Android **Storage Access Framework (SAF)**
//!      system file picker (`ACTION_OPEN_DOCUMENT`, `ACTION_CREATE_DOCUMENT`,
//!      `ACTION_OPEN_DOCUMENT_TREE`).
//!    - Full support for opening and importing `.docx`, `.docm`, `.dotx`, `.dotm`, `.doc`, `.dot`,
//!      `.odt`, `.rtf`, `.html`, `.htm`, `.md`, `.txt`, `.tex`, `.json`, `.xml`, pictures (`.png`,
//!      `.jpg`, `.jpeg`, `.gif`, `.webp`, `.bmp`, `.wmf`, `.emf`), fonts (`.ttf`, `.otf`, `.ttc`,
//!      `.otc`), mail-merge recipient lists (`.csv`, `.tsv`, `.xlsx`, `.ods`), and bibliography
//!      sources (`.xml`), plus saving and exporting to `.docx`, `.docm`, `.dotx`, `.dotm`, `.pdf`,
//!      `.odt`, `.rtf`, `.html`, `.md`, `.txt`, `.tex`, `.png`, `.json`, and `.xml`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod file_dialog;
pub mod gpu_startup;
pub mod permissions;
pub mod services;
pub mod shell;
pub mod touch_twins;

#[cfg(target_os = "android")]
mod android_entry;

/// Host-visible entry symbol used by non-Android builds and tests to verify the crate links cleanly.
pub fn wordcraft_android_abi_version() -> u32 {
    1
}
