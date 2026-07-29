# ShiPing 跨平台架构

## 目标

Slint UI 和录制业务核心不直接依赖 HWND、GDI、WASAPI、Media Foundation 等平台类型。每个平台通过同一组端口提供目标选择、视频采集、音频采集、编码封装和桌面集成能力。

```text
Slint UI
├─ Application / recording core
│  ├─ TargetSelection
│  ├─ VideoCapture
│  ├─ AudioCapture
│  └─ MediaWriter
└─ DesktopIntegration
   ├─ Windows backend
   ├─ macOS backend (规划中)
   └─ Linux backend (规划中)
```

## 当前代码边界

- `src/domain.rs`
  - 平台无关的 `Bounds`、`RecordingTarget`、候选显示器/窗口和音频源类型。
  - 原生窗口句柄被封装为不可解释的 `WindowId`，应用层不再识别 HWND。
- `src/ports.rs`
  - 定义五类端口及录制线程运行时、后端工厂。
  - 采集器和编码器在线程内创建、使用并销毁，不要求 `Send`，以兼容 COM 等线程亲和型 API。
- `src/application/recording_service.rs`
  - 负责暂停、停止、时间轴、丢帧策略、音频开关、事件和文件提交。
  - 通过端口使用采集器与编码器，不引用 Windows 具体类型。
- `src/platform/windows`
  - 当前可用实现：Win32 目标枚举、GDI 视频采集、WASAPI 音频采集、Media Foundation MP4 编码和 Win32 桌面集成。
- `src/platform/unsupported.rs`
  - macOS/Linux 的显式未实现后端。它只保证边界完整并返回明确错误，不伪装成可用能力。
- `src/output.rs`
  - GIF 编码和输出文件生命周期属于共享实现，不依赖 Windows 编码器。

## 推荐实施顺序

1. **已完成：建立平台边界**
   - 抽出领域模型和端口。
   - 将 Windows 行为接回端口。
2. **已完成：增加伪平台后端测试**
   - 使用固定视频帧和固定音频样本验证录制核心可替换。
   - 使用可控单调时钟验证暂停、继续和停止不会把暂停时间计入媒体时间轴。
   - 验证视频帧编号、音频采样位置连续，以及运行时音频和光标选项能传递到后端。
   - 注入写入失败并验证 `.partial` 文件被删除，不会留下损坏的最终文件。
3. **macOS 后端**
   - 目标选择优先使用系统 `SCContentSharingPicker`。
   - 采集使用 ScreenCaptureKit 的视频与音频输出。
   - 同时移除配置目录、默认视频目录、快捷键显示和构建依赖中的 Windows 假设。
   - 编码/封装方案需在 macOS 设备上核对当前系统版本、权限和 Rust 绑定后再确定。
4. **Linux 后端**
   - Wayland/沙箱环境优先使用 XDG Desktop Portal `ScreenCast`，从授权的 PipeWire 流读取画面。
   - 全局快捷键优先使用 XDG Desktop Portal `GlobalShortcuts`。
   - 音频和编码/封装方案需针对目标发行版、PipeWire 会话管理器及打包格式验证后再确定。
5. **三平台 CI**
   - Windows、macOS 和 Linux 分别执行格式检查、`cargo check` 和单元测试。
   - CI 只证明编译和单元测试；屏幕权限、声音、多显示器和长时间录制仍需真实设备验证。

跨阶段原则：后端向 UI 报告可用目标类型、系统音频、麦克风、编码格式和快捷键能力；UI 只根据能力启用功能，不按操作系统名称猜测。

## 已确认的官方依据

- Slint 桌面支持范围：<https://docs.slint.dev/latest/docs/slint/guide/platforms/desktop/>
- Apple ScreenCaptureKit：<https://developer.apple.com/documentation/screencapturekit>
- Apple 系统内容选择器：<https://developer.apple.com/documentation/screencapturekit/sccontentsharingpicker>
- XDG ScreenCast Portal：<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html>
- XDG GlobalShortcuts Portal：<https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html>
- Windows WASAPI loopback：<https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording>
- Windows Media Foundation Sink Writer：<https://learn.microsoft.com/en-us/windows/win32/medfound/sink-writer>

## 明确未完成

- macOS 和 Linux 尚无可运行的采集、音频、MP4 编码和桌面集成实现。
- 当前设备只验证 Windows 构建和测试；没有把 Windows 编译通过等同于其他平台可用。
- Windows 当前仍使用 GDI 采集。迁移到 `Windows.Graphics.Capture` 是独立的性能与系统选择器改造，不属于本次依赖倒置。
