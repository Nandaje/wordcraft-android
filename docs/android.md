# WordCraft on Android

WordCraft runs natively on Android (`arm64-v8a`, Android 8.0+ / API 26+) via the `wordcraft-android` crate (`apps/wordcraft-android`) and the Android Gradle wrapper project in `packaging/android/`.

---

## Key Capabilities

### 1. Hardware GPU Acceleration (`apps/wordcraft-android/src/gpu_startup.rs`)
- **Primary Backend**: **Vulkan 1.1+** (`wgpu::Backends::VULKAN`) on Qualcomm Adreno, ARM Mali / Immortalis, Samsung Xclipse, and PowerVR GPUs.
- **Fallback Backend**: **OpenGL ES 3.0+** (`wgpu::Backends::GL`) when Vulkan surface creation is unavailable on older devices.
- **High-Performance Adapter Selection**: Requests `wgpu::PowerPreference::HighPerformance` and ranks discrete/integrated hardware GPUs ahead of virtual/CPU adapters.
- **Full Hardware Buffer & Texture Limits**: Unlocks `adapter.limits().max_buffer_size` and `adapter.limits().max_texture_dimension_2d` on the `wgpu::DeviceDescriptor` so high-DPI multi-page spreads and embedded high-resolution pictures render cleanly on the GPU.
- **Crash-Safe Startup Sentinel**: Writes `gpu-starting.json` before initializing the graphics driver and clears it once the first frame completes. If a mobile driver crashes during initialization, the next launch automatically steps down (`Vulkan` → `OpenGL ES` → `CPU`).
- **Stylus / S-Pen Support**: Enables `Pressure`, `Tilt`, `Orientation`, and `Distance` motion axes on `AndroidApp` for WordCraft's Draw tab ink pens, highlighters, and pressure-sensitive strokes.

### 2. File & Media Storage Permissions (`apps/wordcraft-android/src/permissions.rs`, `file_dialog.rs`, `services.rs`)
- **Manifest & Runtime Permissions**:
  - `android.permission.READ_EXTERNAL_STORAGE` (API 26–32)
  - `android.permission.WRITE_EXTERNAL_STORAGE` (API 26–29)
  - `android.permission.READ_MEDIA_IMAGES` (API 33+)
  - `android.permission.MANAGE_EXTERNAL_STORAGE` (API 30+ All Files Access for opening and saving document folders, templates, fonts, mail-merge spreadsheets, and bibliography sources anywhere on `/storage/emulated/0`)
- **Dual File Access Architecture**:
  1. **In-App Touch-Friendly Android File Browser Modal**: Directly browses `Documents`, `Downloads`, `Pictures`, `Templates`, `Shared Storage`, `App External`, `App Exports`, and `App Internal` with one-tap buttons for **Grant Storage Access**, **All Files Access**, **System File Picker (SAF)**, **Pick Folder (SAF)**, and **+ New Folder**.
  2. **Android Storage Access Framework (SAF)**: Full integration with `Intent.ACTION_OPEN_DOCUMENT`, `Intent.ACTION_CREATE_DOCUMENT`, and `Intent.ACTION_OPEN_DOCUMENT_TREE` via JNI callbacks in `MainActivity.kt`, including automatic `content://` `OutputStream` flushing and `MediaScannerConnection` indexing.
- **Supported Formats**:
  - **Open / Import Documents**: `.docx`, `.docm`, `.dotx`, `.dotm`, `.doc`, `.dot`, `.odt`, `.rtf`, `.html`, `.htm`, `.md`, `.txt`, `.tex`, `.json`, `.xml`
  - **Insert / Change Pictures**: `.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.bmp`, `.wmf`, `.emf`
  - **Fonts**: System fonts (`/system/fonts`, `/product/fonts`) plus user-installed `.ttf`, `.otf`, `.ttc`, `.otc`
  - **Mail Merge & References**: `.csv`, `.tsv`, `.xlsx`, `.ods` recipient lists and `.xml` bibliography sources
  - **Save As / Export / Print to PDF**: `.docx`, `.docm`, `.dotx`, `.dotm`, `.pdf`, `.odt`, `.rtf`, `.html`, `.md`, `.txt`, `.tex`, `.png`, `.xml`, `.json`

---

## Building from Source

### Prerequisites
- Rust 1.95+ with `rustup target add aarch64-linux-android`
- Android SDK (`platforms;android-34`, `build-tools;34.0.0`) and Android NDK `27.0.12077973`
- JDK 17 and Gradle 8.9+

### Build Commands

```bash
# Run host unit & roundtrip tests for the Android crate
cargo test -p wordcraft-android --lib

# Build libwordcraft_android.so (arm64-v8a) and assemble the release APK
./packaging/android/package.sh
```

The resulting APK is written to:
`dist/release/wordcraft-0.11.0-android-arm64-v8a.apk`
