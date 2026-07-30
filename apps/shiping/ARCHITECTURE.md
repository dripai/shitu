# ShiPing 跨平台架构

## 目标

Slint UI 和录制业务核心不直接依赖 HWND、GDI、WASAPI、Media Foundation、ScreenCaptureKit 或 PipeWire 类型。各平台通过同一组端口提供目标选择、视频采集、音频采集、编码封装和桌面集成能力。

```text
Slint UI
├─ Application / recording core
│  ├─ TargetSelection
│  ├─ VideoCapture
│  ├─ AudioCapture
│  ├─ MediaWriter
│  └─ RecordingCapabilities
└─ DesktopIntegration
   ├─ Windows backend
   ├─ macOS backend
   └─ Linux backend
```

## 当前代码边界

- `src/domain.rs`：平台无关的坐标、录制目标、候选显示器/窗口和音频源类型。原生窗口标识封装为不可解释的 `WindowId`。
- `src/ports.rs`：定义目标选择、视频、音频、媒体写入、录制线程运行时和桌面集成端口，并通过 `RecordingCapabilities` 报告平台能力。
- `src/application/recording_service.rs`：统一处理暂停、继续、停止、时间轴、丢帧、运行时选项和输出提交；伪后端单元测试验证该核心不依赖具体平台。
- `src/platform/windows`：Win32 目标枚举、GDI 视频、WASAPI 音频、Media Foundation MP4 和 Shell 集成。
- `src/platform/macos.rs`：ScreenCaptureKit 显示器/窗口枚举、画面与音频采集，`open` 桌面集成。
- `src/platform/linux.rs`：XDG ScreenCast Portal 授权、PipeWire 视频流、`pw-record` 麦克风和 `xdg-open` 桌面集成。
- `src/platform/ffmpeg.rs`：macOS/Linux 共用的 FFmpeg MP4 适配器，写入原始 BGRA/PCM 临时数据后统一编码并保证失败清理。
- `src/output.rs`：三平台共用的 GIF 编码和输出文件生命周期。

## 实施状态

1. **已完成：建立平台边界**
   - 领域模型和端口不暴露平台类型。
   - Windows 原有行为已接回端口。
2. **已完成：伪平台后端测试**
   - 固定视频帧、音频样本和单调时钟验证暂停、继续、停止和连续媒体时间轴。
   - 验证运行时选项传递以及写入失败时删除 `.partial` 文件。
3. **已完成代码实现：macOS 后端**
   - 使用 ScreenCaptureKit 8.0.1 的 `SCShareableContent`、`SCContentFilter`、`SCScreenshotManager` 和 `SCStream`。
   - 保留 ShiPing 现有目标选择交互，因此目标列表使用 `SCShareableContent`，未叠加第二套 `SCContentSharingPicker` 界面。
   - 麦克风输出使用 macOS 15 API；macOS 构建目标因此固定为 15.0。
   - MP4 通过外部 `ffmpeg` 的 H.264/AAC 编码器生成；缺少命令时明确报错，不切换到其他编码路径。
4. **已完成代码实现：Linux Wayland/Flatpak 后端**
   - 使用 XDG Desktop Portal `ScreenCast` 取得用户授权和 PipeWire 节点，使用 pipewire-rs 读取 BGRA/BGRx/RGBA/RGBx 视频。
   - 窗口和显示器最终选择由系统 Portal 完成；区域模式在已授权显示器流上裁剪。
   - 麦克风通过 PipeWire 官方工具 `pw-record` 输出 48 kHz 双声道 S16 PCM。
   - PipeWire 没有可移植的“默认系统声音监听源”，当前能力协商明确关闭 Linux 系统声音；没有隐式选择节点。
   - 当前只实现 Portal 路径，不另设 X11 捕获回退。
5. **已完成配置：三平台 CI 与发布打包**
   - Windows、macOS 15 和 Ubuntu 24.04 分别执行 `cargo check --locked --package shiping` 与 `cargo test --locked --package shiping`。
   - 独立任务执行 `cargo fmt --all -- --check`。
   - 版本标签发布时，Windows 继续同时打包 ShiTu 与 ShiPing；Linux x64、macOS ARM64 和 macOS x64 只打包 ShiPing。
   - Linux 产物为带桌面入口和图标的 `.tar.gz`；macOS 产物为带 `Info.plist`、权限用途说明和图标的 `.app.zip`。

## 已确认限制

- macOS 最低系统版本为 15.0；录屏和麦克风仍受系统权限控制。
- macOS/Linux 的 MP4 输出要求 `ffmpeg` 可从 `PATH` 启动。
- Linux 需要 XDG Desktop Portal、PipeWire 及 `pw-record`。Portal 会话开始后不能动态切换光标捕获；系统声音和鼠标点击高亮在 UI 中禁用。
- GitHub Release 中的 macOS `.app.zip` 使用 ad-hoc 签名，没有 Developer ID 签名或 Apple 公证；它是测试产物，不是已完成正式分发认证的安装包。
- Linux `.tar.gz` 是便携归档，不是 Flatpak；桌面集成文件随包提供，但运行时依赖由目标系统提供。
- Windows 的系统声音、麦克风和点击高亮继续可用；Windows 仍使用 GDI 采集。
- 全局快捷键仍使用 `global-hotkey`。Wayland 合成器是否允许注册由运行环境决定；本阶段没有增加另一套 Portal GlobalShortcuts 实现。
- CI 只证明对应平台能够编译且单元测试通过，不证明屏幕权限、音频设备、多显示器、缩放和长时间录制在真实设备上可用。

## 官方依据

- [Slint 桌面平台](https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/)
- [Apple ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)
- [Apple SCContentSharingPicker](https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker)
- [XDG ScreenCast Portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html)
- [XDG GlobalShortcuts Portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html)
- [pipewire-rs](https://pipewire.pages.freedesktop.org/pipewire-rs/pipewire/)
- [Windows WASAPI loopback](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [Windows Media Foundation Sink Writer](https://learn.microsoft.com/en-us/windows/win32/medfound/sink-writer)

## 验证边界

- Windows：当前开发设备执行编译、单元测试和 Clippy。
- macOS/Linux：由各自 GitHub Actions runner 执行编译和单元测试。
- 未进行 macOS/Linux 真实设备录制测试；这符合当前阶段“只验证编译和单元测试”的范围。
