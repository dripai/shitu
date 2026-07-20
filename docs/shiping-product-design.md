# 拾屏产品设计基线

> 当前阶段：Windows 首版录屏主流程已实现，等待扩大设备与显示环境验证
> 产品定位：轻量、离线优先的桌面录屏工具

## 1. 核心目标

拾屏服务于会议演示、教程录制、问题复现和临时分享。核心流程必须保持为：选择目标、确认声音、开始录制、停止并获得本地文件。

产品不要求登录，不依赖云端，不内置复杂剪辑器，不在首版加入直播和内容管理。

## 2. 首版功能范围

| 能力 | 状态 | 产品说明 |
| --- | --- | --- |
| 屏幕录制 | 已实现 | 单屏直接使用该显示器；多屏按主屏优先列出每个显示器，只能选择其中一个 |
| 窗口录制 | 已实现 | 开始前选择一个可见窗口；窗口被遮挡的部分会按屏幕实际可见内容录制 |
| 区域录制 | 已实现 | 在虚拟桌面覆盖层框选固定屏幕区域 |
| 开始、暂停、继续、停止 | 已实现 | 所有录制方式使用同一控制流程，暂停时计时和媒体时间戳同步冻结 |
| 系统声音 | 已实现 | WASAPI 回环采集，可独立开关 |
| 麦克风 | 已实现 | WASAPI 采集，可独立开关，不默认启用 |
| 清晰度 | 已实现 | 自动、720p、1080p、原始分辨率；保持比例且不放大源画面 |
| 帧率 | 已实现 | 30 FPS、60 FPS；默认 30 FPS |
| 鼠标指针 | 已实现 | 默认显示，可选鼠标点击高亮 |
| 开始倒计时 | 已实现 | 默认 3 秒，可设为关闭、3 秒或 5 秒 |
| 本地保存 | 已实现 | 固定保存 MP4；成功完成前使用临时文件，结束后原子提交 |
| 录制状态与计时 | 已实现 | 倒计时、录制、暂停、完成和错误状态均在紧凑控制条显示 |
| 全局快捷键与托盘 | 已实现 | 默认 F10 开始、F11 暂停/继续、F12 停止；三项均可修改或禁用，托盘可恢复主窗口并控制录制 |
| 首选项 | 已实现 | 集中配置常规、录制和快捷键；系统快捷键冲突会定位到具体项目，失败时不保存新配置 |
| 异常恢复 | 规划中 | 尽量保留可恢复的未完成录制 |

“已实现”表示项目代码中已有直接实现路径，不等同于已经覆盖所有声卡、编码器、显示器和缩放组合。

## 3. 界面结构

主程序只保留一个紧凑的无标题栏圆角工具条窗口，录制前、录制中和暂停状态均在同一布局内切换：

1. 左侧固定显示录制状态和计时器，计时器下方显示当前来源、清晰度、帧率或简短状态。
2. 右侧依次提供来源、清晰度、帧率、系统声、麦克风和开始/停止。
3. 未录制时，“范围”按钮提供屏幕、窗口和区域；屏幕子菜单实时列出“显示器 1、显示器 2……”及分辨率，不提供合并录制所有显示器。清晰度通过紧凑下拉选择自动、720p、1080p 或原始分辨率；帧率只有 30/60 FPS 两项，继续按点击循环。
4. 录制中，来源按钮切换为暂停/继续；清晰度、帧率和声音设置自动收起，只保留状态、计时、暂停和停止，暂停或停止后再展开。
5. 停止和保存结果继续显示在计时器下方，不打开完成弹窗。
6. 鼠标细项、倒计时、保存目录和快捷键不常驻占用工具条空间；右键菜单只保留录制控制、首选项、打开保存目录和退出。
7. 无标题栏窗口的所有非交互空白区域均可拖动；空闲状态不重复显示“就绪”，只保留参数摘要和“开始”动作。
8. 窗口和区域选择器将操作提示固定在左下角；候选窗口或已拖选区域保持清晰，外围压暗并显示深色阴影边框。
9. 首选项采用 600×400px 紧凑对话框，使用左侧纵向分类导航和右侧设置内容区；右侧各页统一左侧内容起点、96px 标签列、32px 设置行和 8px 行间距，提示与错误只显示在底部独立状态栏。

界面基于 Slint 1.17，并统一使用 `fluent-dark` 样式。已核对原生 `Dialog`、`Button`、`TabWidget`、`LineEdit`、`Switch`、`ComboBox`、`Palette`、`TouchArea`、`FocusScope`、`PopupWindow`、`ContextMenuArea`、`Menu`、`MenuItem` 和 `MenuSeparator`，并检查了 `shi-ui` 现有的 `StatusBar`。原生 `Button` 的公开结构固定为横向图标与文字，不能表达参考设计所需的“圆形图标 + 下方标签 + 紧凑选中态”；原生 `ComboBox` 的 Fluent 实现最小宽度为 160px，也不能放入 46×57px 的快捷操作位；公共 `StatusBar` 的固定高度和状态结构不能容纳录制状态、计时与参数摘要。因此只为六个快捷操作、录制状态区和原始快捷键捕获框自定义必要视觉或事件结构。首选项使用原生 `Dialog`、`TabWidget` 和标准控件；范围、清晰度和应用右键菜单使用原生 `ContextMenuArea`、`Menu` 与 `MenuItem`，由 Slint 后端处理键盘、失焦关闭、屏幕边缘与缩放；无标题栏拖动通过 Slint 的 Winit 访问器调用 Winit `Window::drag_window()`，不再直接发送 Win32 窗口消息。

## 4. 默认值

- 录制目标：整个屏幕。
- 清晰度：1080p。
- 帧率：30 FPS。
- 系统声音：开启。
- 麦克风：关闭。
- 显示鼠标：开启。
- 突出鼠标点击：关闭。
- 开始倒计时：3 秒。
- 开始后自动最小化：关闭。
- 停止并保存后打开保存目录：关闭。
- 全局快捷键：F10 开始、F11 暂停/继续、F12 停止；均可修改或单独禁用。
- 输出格式：首版固定为 MP4，视频使用 Media Foundation H.264，启用声音时使用 AAC。

## 5. 明确不进入首版

- 摄像头画中画。
- 直播推流。
- 云端上传和账号体系。
- 多轨时间线、字幕和专业剪辑。
- GIF 录制。
- 定时任务和批量录制。
- 水印、贴纸和实时标注。

这些能力会显著增加界面和录制链路复杂度，只有在基础录制稳定后再单独评估。

## 6. 源码分层

- `ui/main-window.slint` 与 `ui/preferences-window.slint` 只负责视觉、命中测试、焦点和原始鼠标键盘输入，并通过语义回调表达“开始录制”“应用设置”“捕获快捷键”等用户意图。
- `src/ui/controller.rs` 绑定主窗口与托盘回调，将用户意图转换为业务状态变更，并把录制事件渲染回界面；`src/ui/hotkeys.rs` 负责全局快捷键的整组注册、事件转发和失败回滚。
- `src/application/state.rs` 保存录制业务唯一可变状态，包括配置、录制目标、录制器、倒计时任务和最近输出，不持有 Slint 组件或平台窗口对象。
- `src/application/recording_service.rs` 编排录制生命周期；`src/platform/windowing.rs` 封装基于 Winit 的桌面窗口交互；`src/platform/windows/` 集中实现窗口句柄、目标枚举、GDI 画面采集、WASAPI 音频、Media Foundation 编码和系统 Shell 调用。
- `src/main.rs` 只负责模块装配、生成 Slint 类型和启动 UI Controller。

当前分层已经隔离 Windows 实现，但尚未定义一套凭空假设的跨平台统一接口。增加 macOS 或 Linux 后端前，需要先根据对应平台官方采集、权限、音频和编码能力确定可实现的共同契约，再由 `platform` 选择具体后端。

## 7. 当前实现边界

- 当前仅实现 Windows 路径。屏幕列表通过 `EnumDisplayMonitors` 和 `GetMonitorInfoW` 实时获取，每次展开范围菜单都会刷新，不支持合并多屏录制。画面通过 GDI 读取选定屏幕的当前可见像素，因此窗口录制不是独立的窗口表面捕获：遮挡、屏幕外区域和窗口移动都会反映到录制结果中。
- 工具条的鼠标、键盘、菜单和窗口拖动已经不直接依赖 Win32；Winit 桌面拖动覆盖 Windows、macOS、X11 和 Wayland，但完整应用仍需对应系统的录屏后端才能运行。
- “自动”与“1080p”当前都以 1080p 为上限；720p 和 1080p 均保持原始宽高比、不主动放大，并将编码尺寸调整为偶数。
- 视频通过 Media Foundation Sink Writer 写入 H.264/MP4，并使用 CBR 控制文件体积：720p30 为 2.5 Mbps、720p60 为 4 Mbps、1080p30 为 4 Mbps、1080p60 为 6 Mbps；原始分辨率按像素数量同比增加，30 FPS 上限 12 Mbps、60 FPS 上限 18 Mbps。
- 声音以 48 kHz、双声道、16 位 PCM 输入 Sink Writer，由系统编码为 192 kbps AAC。系统声音与麦克风同时开启时在应用内混音。
- 音频端点、媒体编码器或输出目录不可用时明确报错，不切换到其他采集源或生成伪文件。完成前使用 `.partial.mp4`，只有编码器正常结束才改名为最终文件。
- 输出目录、来源、清晰度、帧率、声音、鼠标、倒计时、自动最小化、保存后打开目录和三项快捷键持久化到 `%APPDATA%\ShiPing\config.json`。
- 托盘使用 Slint `SystemTrayIcon`；全局快捷键使用 `global-hotkey` 0.8.0。快捷键允许单独禁用，普通按键必须带 Ctrl、Alt、Shift 或 Win，F1–F24 可单独使用。应用新设置时先校验格式与内部重复，再整组替换；任一键被占用都会撤销新注册并恢复旧快捷键，只有整组成功后才保存配置。
- 异常恢复仍为规划中，首版没有备用实现路径。

## 8. 官方依据

- [Media Foundation：使用 Sink Writer 编码视频](https://learn.microsoft.com/en-us/windows/win32/medfound/tutorial--using-the-sink-writer-to-encode-video)
- [`MFCreateSinkWriterFromURL`](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-mfcreatesinkwriterfromurl) 与 [`IMFSinkWriter::SetInputMediaType`](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-imfsinkwriter-setinputmediatype)
- [WASAPI 回环录制](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [AAC 编码器](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-encoder) 与 [AAC 媒体类型](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-media-types)
- [`EnumDisplayMonitors`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaymonitors) 与 [`MONITORINFO`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-monitorinfo)
- [Slint `TouchArea`](https://docs.slint.dev/latest/docs/slint/reference/gestures/toucharea/)、[`FocusScope`](https://docs.slint.dev/latest/docs/slint/reference/keyboard-input/focusscope/) 与 [`ContextMenuArea`](https://docs.slint.dev/latest/docs/slint/reference/window/contextmenuarea/)
- [Slint `Dialog`](https://docs.slint.dev/latest/docs/slint/reference/window/dialog/)、[`TabWidget`](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/tabwidget/) 与 [`StandardButton`](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/standardbutton/)
- [Slint `SystemTrayIcon`](https://docs.slint.dev/latest/docs/slint/reference/window/systemtrayicon/) 与 [`Window.minimized`](https://docs.slint.dev/latest/docs/slint/reference/window/window/#minimized)
- [`global-hotkey` 0.8.0 `HotKey`](https://docs.rs/global-hotkey/0.8.0/global_hotkey/hotkey/struct.HotKey.html) 与 [`GlobalHotKeyManager`](https://docs.rs/global-hotkey/0.8.0/global_hotkey/struct.GlobalHotKeyManager.html)
- [Slint `WinitWindowAccessor`](https://docs.slint.dev/latest/docs/rust/slint/winit_030/trait.WinitWindowAccessor) 与 [Winit `Window::drag_window`](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.drag_window)

## 9. 验证状态

- 已通过：`cargo check -p shiping`、常规单元测试、短时真实 MP4 录制测试以及 Debug/Release 构建。
- 当前设备已验证：主界面与右键菜单、3 秒倒计时、录制/暂停/继续/停止、结果文件生成、Escape 与右键取消目标选择、可见窗口选择和区域拖选。
- 尚未覆盖：首选项窗口的人工交互、真实第三方快捷键冲突、多显示器含负坐标、不同 DPI/缩放组合、60 FPS 持续负载、不同音频端点格式、无 AAC/H.264 编码器设备、长时间录制和发布构建安装包。编译通过不能替代这些设备矩阵验证。
