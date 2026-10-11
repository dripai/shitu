# ShiTu

[简体中文](README.zh-CN.md)

A local Windows screenshot, annotation, OCR, and pinning tool. No account is required.


ShiTu is built for the small screenshot tasks that happen all day: copying part of a document, explaining a UI issue, extracting text, or keeping a reference visible while you work.

- Capture a screen region or select a visible window.
- Annotate with pen, rectangle, ellipse, arrow, text, eraser, and mosaic tools; select, move and resize annotations, with undo and redo.
- Copy immediately, save as PNG/JPEG, or enable automatic saving.
- Browse PNG/JPEG folders in Gallery with thumbnails or a file list. Double-click for an internal preview with left/right navigation, zoom/pan, rotation and Esc to close. Preview pixels are cached up to 10 images and 100MB, then cleared on close; rotation does not modify files. Thumbnails load for visible rows. Rename images, move them by dragging to a folder, or send them to the Recycle Bin. Ctrl-click toggles selection, Shift-click selects a range; right-click or Delete confirms moving the selected group to the Recycle Bin. The context menu also offers Delete, with an explicit permanent-deletion warning and confirmation; it bypasses the Recycle Bin. Removing a folder from Gallery keeps its files intact.
- Pin screenshots above other windows and adjust zoom, opacity, and always-on-top behavior.
- Recognize text locally with Windows system OCR and copy the result.
- Start a capture from the system tray or the default `Ctrl+Alt+C` shortcut.
- Opening About checks for updates in the background; automatic check failures stay silent, while manual failures are shown inline. Update and restart downloads, verifies and installs newer Windows x64 portable releases without a confirmation popup. Save unfinished work first. Packaged installations use their original distribution channel.
- Follow the system theme and language, or choose Simplified Chinese, English, Japanese, Korean, French, German, Spanish, Portuguese, Russian, or Hindi in General → Language. Click Save to keep the language after restarting.

The UI now uses GPUI Kit 0.7.1. Migration scope, source references and validation are tracked in [MIGRATION.md](MIGRATION.md).

## Download and support

- [Download the latest release](https://github.com/dripai/shitu/releases)
- [Report a problem](https://github.com/dripai/shitu/issues)
- [Privacy policy](PRIVACY.en.md)

ShiPing is maintained independently at [dripai/ShiPing](https://github.com/dripai/ShiPing).

## Build locally

Use Windows, Rust 1.96, Visual Studio C++ Build Tools and the Windows SDK. Git Bash is needed only for start.sh. The root Cargo package builds ShiTu:

```bash
./start.sh dev
./start.sh build
cargo test --locked
```

Version tags publish only ShiTu Windows x64 packages.

## Current boundaries

- The application targets Windows. This migration does not verify every Windows version or GPU driver; see the validation checklist. The current Store manifest requires Windows 11 24H2 (build 26100) and Windows App Runtime 1.8.
- Enhanced Windows AI OCR remains experimental and unverified on supported NPU hardware.
- `src/`: application and platform code; `assets/`: icons and reference images; `translations/`: ten PO catalogs; `packaging/`: Store manifest; `tools/`: packaging, catalog compilation and AI binding generation.
- The Slint UI, patched Slint vendor code, shared workspace crates and unimplemented audio placeholder have been removed.
- The previous UI screenshot is retained at `assets/shitu_01.jpg` as a historical reference.
