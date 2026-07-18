# ShiTu (拾图)

[简体中文](README.zh-CN.md)

ShiTu is a lightweight, offline-first screenshot tool for Windows, built with Rust and Slint.

## Features

- Capture a screen region or select a window.
- Annotate images with pen, rectangle, arrow, text, and eraser tools; undo and redo are supported.
- Copy to the clipboard, save as PNG/JPEG, or enable automatic saving.
- Pin screenshots on screen with zoom, opacity, always-on-top, and image replacement controls.
- Use Windows system OCR to recognize text in a capture or pinned image and copy the result.
- Use the system tray or the default `Ctrl+Alt+C` global shortcut to start a capture.
- Follow the system theme, or use Simplified Chinese or English.

## Get ShiTu

- [GitHub Releases](https://github.com/dripai/shitu/releases)
- [Report an issue](https://github.com/dripai/shitu/issues)
- [Privacy policy](PRIVACY.md)

## Build locally

Windows 10/11 and a stable Rust toolchain are required:

```powershell
cargo run --release
```

The release executable is `ShiTu.exe`.

## OCR notes

Basic OCR uses Windows-provided local system capabilities. Screenshots and OCR results are not uploaded by ShiTu.

The project also includes an enhanced Windows AI OCR path. It requires compatible Windows, package identity, Windows App Runtime, and hardware. It has not yet been verified on a supported NPU device and should not be considered a verified feature.

See the [product design document](docs/product-design.md) for the current product scope, platform boundaries, and implementation status.
