# WordCraft Android Packaging (`packaging/android`)

This directory contains the Android Gradle project (`ai.storyteller.wordcraft`) that packages `libwordcraft_android.so` (built from `apps/wordcraft-android`) into an installable Android APK (`arm64-v8a`, Android 8.0+ / API 26+).

## Features

1. **Hardware GPU Acceleration (`wgpu` Vulkan 1.1+ & OpenGL ES 3.0+)**:
   - Enables hardware-accelerated UI and page texture presentation via `wgpu::Backends::VULKAN | wgpu::Backends::GL` with `wgpu::PowerPreference::HighPerformance`.
   - Unlocks the mobile GPU's full `max_buffer_size` and `max_texture_dimension_2d` limits for crisp high-DPI page rendering.
   - Includes automatic crash-sentinel fallback (`Vulkan` -> `OpenGL ES` -> `CPU`).
   - Enables stylus / S-Pen pressure, tilt, orientation, and hover distance axes for WordCraft's Draw tab ink pens and highlighters.

2. **File & Media Storage Permissions for Import & Export**:
   - Declares and requests `READ_EXTERNAL_STORAGE`, `WRITE_EXTERNAL_STORAGE`, `READ_MEDIA_IMAGES`, and `MANAGE_EXTERNAL_STORAGE` (All Files Access on Android 11+).
   - Integrates Android's **Storage Access Framework (SAF)** (`ACTION_OPEN_DOCUMENT`, `ACTION_CREATE_DOCUMENT`, `ACTION_OPEN_DOCUMENT_TREE`) alongside an in-app touch-friendly **Android File Browser** modal.
   - Supports importing and opening `.docx`, `.docm`, `.dotx`, `.dotm`, `.doc`, `.dot`, `.odt`, `.rtf`, `.html`, `.htm`, `.md`, `.txt`, `.tex`, `.json`, `.xml`, pictures (`.png`, `.jpg`, `.jpeg`, `.gif`, `.webp`, `.bmp`, `.wmf`, `.emf`), fonts (`.ttf`, `.otf`, `.ttc`, `.otc`), mail-merge recipient lists (`.csv`, `.tsv`, `.xlsx`, `.ods`), and bibliography sources (`.xml`), plus saving and exporting to `.docx`, `.docm`, `.dotx`, `.dotm`, `.pdf`, `.odt`, `.rtf`, `.html`, `.md`, `.txt`, `.tex`, `.png`, `.json`, and `.xml`.

## Building

```bash
./packaging/android/package.sh
```

See [docs/android.md](../../docs/android.md) for full build and architecture details.
