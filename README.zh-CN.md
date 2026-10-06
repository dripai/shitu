# 拾图（ShiTu）

[English](README.md)

本地 Windows 截图工具，提供标注、OCR 与钉住功能，无需账号。

![拾图应用设置界面](images/shitu_01.jpg)

拾图服务于每天都会发生的轻量截图任务：复制文档片段、说明界面问题、提取文字，或把参考内容留在桌面上随时对照。

- 截取屏幕区域，或直接选择可见窗口。
- 使用画笔、矩形、箭头、文字和橡皮擦完成标注，支持撤销与重做。
- 立即复制、另存为 PNG/JPEG，或开启自动保存。
- 将截图钉在其他窗口上方，并调整缩放、不透明度和置顶状态。
- 使用 Windows 系统 OCR 在本地识别文字并复制结果。
- 通过系统托盘或默认 `Ctrl+Alt+C` 快捷键立即开始截图。
- 支持跟随系统主题、English 与简体中文界面。

## 下载与反馈

- [下载最新版本](https://github.com/dripai/shitu/releases)
- [反馈问题](https://github.com/dripai/shitu/issues)
- [隐私政策](PRIVACY.md)

拾屏已迁入独立项目 [dripai/ShiPing](https://github.com/dripai/ShiPing)。

## 本地构建

需要 Windows 10/11、Git Bash 和 Rust，工作区默认运行拾图：

```bash
./start.sh dev
./start.sh build
cargo test --workspace --locked
```

版本标签只发布拾图 Windows x64 包。

## 当前边界

- 拾图支持 Windows 10/11，本次迁移不增加平台支持。
- Windows AI OCR 增强路径仍为实验能力，尚未在受支持的 NPU 设备上验证。
- 应用位于 `apps/shitu`；`crates/shi-foundation`、`crates/shi-ui` 是内部模块。
- `apps/shiyin` 保留为规划中的录音工具，录音尚未实现。
