# 拾图（ShiTu）

[English](README.md)

本地 Windows 截图工具，提供标注、OCR 与钉住功能，无需账号。


拾图服务于每天都会发生的轻量截图任务：复制文档片段、说明界面问题、提取文字，或把参考内容留在桌面上随时对照。

- 截取屏幕区域，或直接选择可见窗口。
- 使用画笔、矩形、圆形/椭圆、箭头、文字、橡皮擦与马赛克标注，支持选择、移动、缩放、撤销和重做。
- 立即复制、另存为 PNG/JPEG，或开启自动保存。
- 图库支持多目录、缩略图/列表浏览、文件名搜索和排序；双击在软件内预览，支持左右切图、缩放/拖动、旋转及 Esc 关闭。预览缓存最多 10 张且不超过 100MB 像素数据，关闭预览清空；旋转不修改原文件。缩略图按可见区域加载；可重命名图片、拖到目录中移动或移入回收站。支持 Ctrl 点击多选、Shift 点击区间选择，右键或 Delete 确认后批量移入回收站；右键“删除”经永久删除提示并确认后直接删除，不经过回收站。从图库移除目录不会删除其中的文件。
- 将截图钉在其他窗口上方，并调整缩放、不透明度和置顶状态。
- 使用 Windows 系统 OCR 在本地识别文字并复制结果。
- 通过系统托盘或默认 `Ctrl+Alt+C` 快捷键立即开始截图。
- 进入“关于”页自动检查新版，联网失败静默；手动检查失败才显示原因。发现新版后点击“更新并重启”可下载、校验并更新 Windows x64 便携版，无弹窗确认；更新前请保存未完成内容。打包安装版通过原渠道更新。
- 支持跟随系统主题和语言；界面提供简体中文、英语、日语、韩语、法语、德语、西班牙语、葡萄牙语、俄语和印地语。在“常规 → 界面语言”选择并保存，重启后保留。

UI 已切换到 GPUI Kit 0.7.1，迁移范围、官方依据和验证进度见 [MIGRATION.md](MIGRATION.md)。

## 下载与反馈

- [下载最新版本](https://github.com/dripai/shitu/releases)
- [反馈问题](https://github.com/dripai/shitu/issues)
- [隐私政策](PRIVACY.md)

拾屏已迁入独立项目 [dripai/ShiPing](https://github.com/dripai/ShiPing)。

## 本地构建

需要 Windows、Rust 1.96、Visual Studio C++ Build Tools 和 Windows SDK。仅 start.sh 需要 Git Bash；根 Cargo 包直接构建拾图：

```bash
./start.sh dev
./start.sh build
cargo test --locked
```

版本标签只发布拾图 Windows x64 包。

## 当前边界

- 目标平台为 Windows；此次迁移没有验证所有 Windows 版本及 GPU 驱动。当前 Store 清单要求 Windows 11 24H2（build 26100）和 Windows App Runtime 1.8。
- Windows AI OCR 增强路径仍为实验能力，尚未在受支持的 NPU 设备上验证。
- `src/`：应用和平台实现；`assets/`：图标及参考图；`translations/`：十种语言；`packaging/`：Store 清单；`tools/`：打包、翻译编译和 AI 绑定生成。
- 已移除 Slint UI、Slint vendor 补丁、共享分包和未实现的录音占位项目。
- `assets/shitu_01.jpg` 保留为旧版界面的历史参考图。
