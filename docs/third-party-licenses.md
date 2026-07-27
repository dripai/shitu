# 第三方许可

截图工具使用以下主要开源项目。完整依赖列表以 `Cargo.lock` 为准。

## Slint

- 项目：https://slint.dev/
- 当前版本：1.17.0
- 用途：桌面用户界面与系统托盘
- 许可：Slint Royalty-free License 2.0 / Slint Software License / GPL-3.0-only

本应用界面使用 Slint 构建。

## Lucide

- 项目：https://lucide.dev/
- 用途：截图与钉住工具图标
- 许可：ISC License

图标源码许可文本保存在 `apps/shitu/src/icons/LICENSE`。

## Microsoft Fluent System Icons

- 项目：https://github.com/microsoft/fluentui-system-icons
- 用途：录屏工具的系统声音与麦克风状态图标
- 许可：MIT License

采用 24px Regular 风格的 Speaker 2、Speaker Off、Mic 与 Mic Off 图标。许可文本保存在 `apps/shiping/ui/icons/FLUENT_SYSTEM_ICONS_LICENSE`。

## Rust 依赖

Rust 依赖包括 `anyhow`、`global-hotkey`、`image`、`raw-window-handle`、`rfd`、`serde`、`serde_json`、`slint` 和 `windows`。各项目保留其原始版权与许可。
