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
| 帧率 | 已实现 | MP4 为 30/60 FPS，GIF 为 10/20 FPS；默认 MP4 30 FPS |
| 鼠标指针 | 已实现 | 默认显示，可选鼠标点击高亮 |
| 开始倒计时 | 已实现 | 默认 3 秒，可设为关闭、3 秒或 5 秒 |
| 本地保存 | 已实现 | 可选 MP4 或无声 GIF；成功完成前使用同格式临时文件，结束后原子提交 |
| 录制状态与计时 | 已实现 | 倒计时、录制、暂停、完成和错误状态均在紧凑控制条显示 |
| 全局快捷键与托盘 | 已实现 | 默认 F10 开始、F11 暂停/继续、F12 停止；三项均可直接修改或清除，托盘可恢复主窗口并控制录制 |
| 中英文界面 | 已实现 | 默认显示英文；首选项可切换 English/简体中文，保存后持久化，取消或保存失败时恢复已保存语言 |
| 首选项 | 已实现 | 集中配置常规、录制和快捷键；右下角统一提供“恢复默认、取消、保存”，保存后关闭；状态栏空闲时隐藏，有有效状态时显示在最底部 |
| 应用图标 | 已实现 | 主窗口、首选项、选择窗口、任务栏、托盘与 Windows 可执行文件统一使用拾屏录制图标；桌面 EXE 或快捷方式读取可执行文件内嵌图标 |
| 异常恢复 | 规划中 | 尽量保留可恢复的未完成录制 |

“已实现”表示项目代码中已有直接实现路径，不等同于已经覆盖所有声卡、编码器、显示器和缩放组合。

## 3. 界面结构

主程序只保留一个紧凑的无标题栏圆角工具条窗口，录制前、录制中和暂停状态均在同一布局内切换：

1. 左侧只保留录制目标图标、计时器和参数摘要，不再单独放置一个与停止按钮重复的红色录制状态圆点。计时器与摘要使用合并后的 209.5px 宽度，摘要完整显示当前来源、输出格式、清晰度和帧率。
2. 右侧依次提供来源、清晰度、帧率、系统声、麦克风和开始/停止。清晰度以 `A/720/1080/1:1`、帧率以 `30/60` 直接显示在圆形按钮中央，不再显示底部文字；右侧红色主操作按钮在录制、暂停或准备时于按钮下方显示 `REC`、`暂停` 或 `准备`，同时承担唯一的录制状态指示。光标悬停在整个按钮上时显示包含当前状态和快捷键的完整提示。
3. 未录制时，“范围”按钮提供屏幕、窗口和区域；屏幕子菜单实时列出“显示器 1、显示器 2……”及分辨率，不提供合并录制所有显示器。清晰度通过紧凑下拉选择自动、720p、1080p 或原始分辨率；帧率按当前输出格式在 MP4 的 30/60 FPS 或 GIF 的 10/20 FPS 两项间循环。
4. 录制中，来源按钮切换为暂停/继续；清晰度、帧率和声音设置自动收起，只保留状态、计时、暂停和停止，暂停或停止后再展开。
5. 停止和保存结果继续显示在计时器下方，不打开完成弹窗；无论通过快捷键、托盘还是窗口内按钮停止，都会立即取消最小化并恢复主窗口。
6. 鼠标细项、倒计时、保存目录和快捷键不常驻占用工具条空间；右键菜单只保留录制控制、首选项、打开保存目录和退出。
7. 无标题栏窗口的所有非交互空白区域均可拖动；空闲状态不重复显示“就绪”，只保留参数摘要和“开始”动作。
8. 窗口和区域选择器将操作提示固定在左下角；窗口模式不显示桌面蒙版，随光标移动只显示候选窗口的蓝色边框、标题与像素尺寸，单击当前位置直接确认。区域模式始终保持桌面透明。确认后，蓝色边框和“录制窗口”或“录制区域”标签都绘制在录制坐标外侧；指示窗在等待开始、倒计时、录制和暂停期间持续显示，停止或取消录制时移除，切换目标时替换或清除。整窗鼠标穿透，不提供会与“停止录制”混淆的关闭按钮。
9. 首选项采用 600×450px 紧凑对话框，左侧“常规、录制、快捷键”固定为三个 36px 单行导航项，不再均分整列高度；右侧各页统一左侧内容起点、96px 标签列、32px 设置行和 8px 行间距。快捷键页为三项操作各提供一个 180px 可点击按键输入框和清除按钮，输入框获得焦点后直接捕获按键组合。操作按钮统一靠右排列为“恢复默认、取消、保存”。空闲时不显示教学式提示；发生恢复默认、预览、错误或录制中不可修改等有效状态时，状态栏显示在所有按钮下方。常规页提供界面语言切换，切换时即时预览，只有“保存”才写入配置。

界面基于 Slint 1.17，并统一使用 `fluent-dark` 样式。已核对原生 `Dialog`、`Button`、`TabWidget`、`LineEdit`、`Switch`、`ComboBox`、`Palette`、`TouchArea`、`FocusScope`、`Tooltip`、`PopupWindow`、`ContextMenuArea`、`Menu`、`MenuItem`、`MenuSeparator` 和 Slint 捆绑翻译 API，并检查了 `shi-ui` 现有的 `StatusBar`。原生 `Button` 的公开结构固定为横向图标与文字，不能表达参考设计所需的“圆形图标 + 可选状态值 + 紧凑选中态”；原生 `ComboBox` 的 Fluent 实现最小宽度为 160px，也不能放入 46×57px 的快捷操作位；原生纵向 `TabWidget` 的标签栏会占满内容高度，公开 API 没有单项行高属性；Slint 1.17 的 `Dialog` 会把带 `dialog-button-role` 的直接子按钮固定排在内容之后，不能再把状态栏放到按钮下方；Slint `Window` 没有公开的鼠标穿透属性；公共 `StatusBar` 的固定高度和状态结构不能容纳录制状态、计时与参数摘要。因此只为六个快捷操作、录制状态区和原始快捷键捕获框自定义必要视觉或事件结构。悬浮提示直接使用原生 `Tooltip`，不自定义弹窗；首选项保留原生 `Dialog`、`ComboBox` 和标准 `Button`，仅显式排列底部按钮与状态栏，并用三个原生可选 `Button` 替换纵向标签导航；范围、清晰度和应用右键菜单使用原生 `ContextMenuArea`、`Menu` 与 `MenuItem`，由 Slint 后端处理键盘、失焦关闭、屏幕边缘与缩放；目标指示窗仅绘制录制矩形外侧的四条边和标签，显示后通过 Slint 1.17 的异步 `WinitWindowAccessor::winit_window()` 等待原生窗口创建，再调用 Winit `Window::set_cursor_hittest(false)` 实现鼠标穿透；中英文文案使用 Slint `@tr`、构建期捆绑 PO 翻译和运行期 `select_bundled_translation`；无标题栏拖动通过同一访问器调用 Winit `Window::drag_window()`，不再直接发送 Win32 窗口消息。

## 4. 默认值

- 录制目标：整个屏幕。
- 输出格式：MP4。
- 清晰度：1080p。
- 帧率：30 FPS。
- 系统声音：开启。
- 麦克风：关闭。
- 显示鼠标：开启。
- 突出鼠标点击：关闭。
- 开始倒计时：3 秒。
- 开始后自动最小化：关闭。
- 停止并保存后打开保存目录：关闭。
- 界面语言：英文；首选项仅提供 English 和简体中文两项。
- 全局快捷键：F10 开始、F11 暂停/继续、F12 停止；均可直接修改或清除，不使用单独的启用开关。
- MP4 视频使用 Media Foundation H.264，启用声音时使用 AAC；GIF 使用 10/20 FPS、无限循环且不包含声音。

## 5. 明确不进入首版

- 摄像头画中画。
- 直播推流。
- 云端上传和账号体系。
- 多轨时间线、字幕和专业剪辑。
- 定时任务和批量录制。
- 水印、贴纸和实时标注。

这些能力会显著增加界面和录制链路复杂度，只有在基础录制稳定后再单独评估。

## 6. 源码分层

- `ui/main-window.slint`、`ui/preferences-window.slint`、`ui/selection-window.slint` 与 `ui/target-indicator-window.slint` 只负责视觉、命中测试、焦点和原始鼠标键盘输入，并通过语义回调表达“开始录制”“保存设置”“捕获快捷键”等用户意图；持久目标指示窗自身不处理输入。
- `src/ui/controller.rs` 绑定主窗口与托盘回调，将用户意图转换为业务状态变更，并把录制事件渲染回界面；`src/ui/hotkeys.rs` 负责全局快捷键的整组注册、事件转发和失败回滚。
- `src/application/state.rs` 保存录制业务唯一可变状态，包括配置、录制目标、录制器、倒计时任务和最近输出，不持有 Slint 组件或平台窗口对象。
- `src/application/recording_service.rs` 编排录制生命周期；`src/output.rs` 根据输出格式适配 MP4 或 GIF 写入器并统一临时文件提交；`src/platform/windowing.rs` 封装基于 Winit 的桌面窗口交互；`src/platform/windows/` 集中实现窗口句柄、目标枚举、GDI 画面采集、WASAPI 音频、Media Foundation MP4 编码和系统 Shell 调用。
- `src/main.rs` 只负责模块装配、生成 Slint 类型和启动 UI Controller。

当前分层已经隔离 Windows 实现，但尚未定义一套凭空假设的跨平台统一接口。增加 macOS 或 Linux 后端前，需要先根据对应平台官方采集、权限、音频和编码能力确定可实现的共同契约，再由 `platform` 选择具体后端。

## 7. 当前实现边界

- 当前仅实现 Windows 路径。屏幕列表通过 `EnumDisplayMonitors` 和 `GetMonitorInfoW` 实时获取，每次展开范围菜单都会刷新，不支持合并多屏录制。画面通过 GDI 读取选定屏幕的当前可见像素，因此窗口录制不是独立的窗口表面捕获：遮挡、屏幕外区域和窗口移动都会反映到录制结果中。
- 持久目标指示窗的透明中心与 GDI 录制坐标完全一致，四条 3px 蓝边和上方标签均位于录制矩形之外，因此不进入当前目标的 `StretchBlt` 源矩形。区域使用固定边界；窗口边界每 50ms 按当前 DWM 扩展框架坐标更新，以跟随移动或缩放。指示窗整窗鼠标穿透，在等待开始、倒计时、录制和暂停期间保留，停止或取消录制时移除；切换目标时替换或清除。目标紧贴虚拟桌面边缘时，位于桌面之外的部分会被系统裁掉；不同 DPI 显示器之间的视觉对齐仍需设备验证。
- 窗口选择覆盖整个 Windows 虚拟桌面，通过 Slint `TouchArea.pointer-event` 的移动事件实时命中 `EnumWindows` 按 Z 序枚举的候选窗口，并使用 `GetWindowTextW` 显示窗口标题。`TouchArea.moved` 只用于按下后的区域拖选，不再承担普通鼠标悬停。按下和释放时都会重新探测当前坐标，因此单击选择不依赖此前是否发生过鼠标移动。
- 工具条的鼠标、键盘、菜单和窗口拖动已经不直接依赖 Win32；Winit 桌面拖动覆盖 Windows、macOS、X11 和 Wayland，但完整应用仍需对应系统的录屏后端才能运行。
- “自动”与“1080p”当前都以 1080p 为上限；720p 和 1080p 均保持原始宽高比、不主动放大，并将编码尺寸调整为偶数。
- 视频通过 Media Foundation Sink Writer 写入 H.264/MP4，并使用 CBR 控制文件体积：720p30 为 2.5 Mbps、720p60 为 4 Mbps、1080p30 为 4 Mbps、1080p60 为 6 Mbps；原始分辨率按像素数量同比增加，30 FPS 上限 12 Mbps、60 FPS 上限 18 Mbps。
- 声音以 48 kHz、双声道、16 位 PCM 输入 Sink Writer，由系统编码为 192 kbps AAC。系统声音与麦克风同时开启时在应用内混音。
- GIF 由 `gif` 0.14.2 直接写入，使用 256 色量化、无限循环和固定 10/20 FPS；GIF 格式不支持音轨，选择 GIF 时系统声音和麦克风会被明确关闭，不启动 WASAPI 或 Media Foundation。
- 音频端点、媒体编码器或输出目录不可用时明确报错，不切换到其他采集源或生成伪文件。完成前按格式使用 `.partial.mp4` 或 `.partial.gif`，只有写入器正常结束才改名为最终文件。
- 界面语言、输出目录、输出格式、来源、清晰度、帧率、声音、鼠标、倒计时、自动最小化、保存后打开目录和三项快捷键持久化到 `%APPDATA%\ShiPing\config.json`。首次启动以及旧配置缺少语言字段时使用英文；首选项中的“保存”执行单一保存事务并关闭窗口，主工具条直接恢复参数摘要，不显示无信息量的保存成功文案；“取消”不写盘。
- 托盘使用 Slint `SystemTrayIcon`；全局快捷键使用 `global-hotkey` 0.8.0。快捷键输入框本身就是设置入口，清空即表示不设置；普通按键必须带 Ctrl、Alt、Shift 或 Win，F1–F12 可单独使用，F13–F24 不接受配置。启动加载既有或默认快捷键时，被其他程序占用的项目自动留空并写回配置，其余项目继续注册；用户在首选项中手动保存新设置时仍先校验格式与内部重复，再整组替换，任一键冲突都会撤销新注册并恢复旧快捷键，只有整组成功后才保存配置。
- 异常恢复仍为规划中，首版没有备用实现路径。

## 8. 官方依据

- [Media Foundation：使用 Sink Writer 编码视频](https://learn.microsoft.com/en-us/windows/win32/medfound/tutorial--using-the-sink-writer-to-encode-video)
- [Media Foundation 容器类型（不包含 GIF）](https://learn.microsoft.com/en-us/windows/win32/medfound/mf-transcode-containertype)
- [`gif` 0.14.2 API](https://docs.rs/gif/0.14.2/gif/)
- [`MFCreateSinkWriterFromURL`](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-mfcreatesinkwriterfromurl) 与 [`IMFSinkWriter::SetInputMediaType`](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-imfsinkwriter-setinputmediatype)
- [WASAPI 回环录制](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [AAC 编码器](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-encoder) 与 [AAC 媒体类型](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-media-types)
- [`EnumDisplayMonitors`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaymonitors) 与 [`MONITORINFO`](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-monitorinfo)
- [Slint `TouchArea`](https://docs.slint.dev/latest/docs/slint/reference/gestures/toucharea/)、[`FocusScope`](https://docs.slint.dev/latest/docs/slint/reference/keyboard-input/focusscope/) 与 [`ContextMenuArea`](https://docs.slint.dev/latest/docs/slint/reference/window/contextmenuarea/)
- [Slint `Dialog`](https://docs.slint.dev/latest/docs/slint/reference/window/dialog/)、[`TabWidget`](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/views/tabwidget/) 与 [`StandardButton`](https://docs.slint.dev/latest/docs/slint/reference/std-widgets/basic-widgets/standardbutton/)
- [Slint `Tooltip`](https://docs.slint.dev/latest/docs/slint/reference/window/tooltip/)
- [Slint 翻译指南](https://docs.slint.dev/latest/docs/slint/guide/development/translations/)（`@tr`、PO 目录和运行期语言选择）
- [Slint `SystemTrayIcon`](https://docs.slint.dev/latest/docs/slint/reference/window/systemtrayicon/) 与 [`Window.minimized`](https://docs.slint.dev/latest/docs/slint/reference/window/window/#minimized)
- [`global-hotkey` 0.8.0 `HotKey`](https://docs.rs/global-hotkey/0.8.0/global_hotkey/hotkey/struct.HotKey.html) 与 [`GlobalHotKeyManager`](https://docs.rs/global-hotkey/0.8.0/global_hotkey/struct.GlobalHotKeyManager.html)
- [Slint `WinitWindowAccessor`](https://docs.slint.dev/latest/docs/rust/slint/winit_030/trait.WinitWindowAccessor)、[Winit `Window::drag_window`](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.drag_window) 与 [`Window::set_cursor_hittest`](https://docs.rs/winit/0.30.13/winit/window/struct.Window.html#method.set_cursor_hittest)

## 9. 验证状态

- 已通过：`cargo test --release --locked -p shiping`（24 项通过、1 项真实录制测试按环境要求忽略，其中 GIF 测试实际写入并重新解码动画及跳帧时序）、`cargo clippy --release --locked -p shiping --all-targets -- -D warnings`、`./start.sh build shiping`、格式检查、英译 PO 覆盖检查和修改文件严格 UTF-8 校验。此前也已完成短时真实 MP4 录制测试。
- 当前设备已验证：主界面与右键菜单、3 秒倒计时、MP4 录制/暂停/继续/停止、结果文件生成、Escape 与右键取消目标选择、可见窗口选择和区域拖选；GIF 当前只完成编解码单元验证。
- 尚未覆盖：真实桌面 GIF 录制、GIF 长时间录制的文件体积与内存占用、首选项新增格式设置与底部状态栏的人工交互、窗口与区域持久边框的实际鼠标穿透、窗口移动跟随及录制成片检查、中英文运行期切换与重启后恢复的人工交互、真实第三方快捷键冲突、多显示器含负坐标、不同 DPI/缩放组合、60 FPS 持续负载、不同音频端点格式、无 AAC/H.264 编码器设备、长时间 MP4 录制和发布构建安装包。编译通过不能替代这些设备矩阵验证。
