# 拾图（ShiTu）

[English](README.md)

拾图是一款轻量、离线优先的 Windows 截图工具，使用 Rust 和 Slint 构建。

## 功能

- 区域截图与窗口选取。
- 画笔、矩形、箭头、文字和橡皮擦标注，支持撤销与重做。
- 复制到剪贴板、另存为 PNG/JPEG 与自动保存。
- 将截图钉在屏幕上，支持缩放、透明度、置顶和图像替换。
- 使用 Windows 系统 OCR 识别选区或钉住图片中的文字，并复制识别结果。
- 系统托盘操作，以及默认 `Ctrl+Alt+C` 全局截图快捷键。
- 跟随系统、简体中文和 English 界面。

## 获取

- [GitHub Releases](https://github.com/dripai/shitu/releases)
- [问题反馈](https://github.com/dripai/shitu/issues)
- [隐私政策](PRIVACY.md)

## 本地构建

需要 Windows 10/11 与 Rust 稳定版工具链：

```powershell
cargo run --release
```

构建后的可执行文件为 `ShiTu.exe`。

## OCR 说明

基础 OCR 使用 Windows 提供的本地系统能力，不上传截图或识别结果。

项目还接入了 Windows AI OCR 增强路径；它需要满足 Windows、包身份、Windows App Runtime 与硬件条件。该增强能力当前尚未在受支持的 NPU 设备上完成实测，不应视为已验证功能。

详细的产品范围、平台边界和开发状态见[产品设计文档](docs/product-design.md)。
