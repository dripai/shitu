# 拾屏产品设计基线

> 当前阶段：Windows 首版录屏主流程已实现，等待扩大设备与显示环境验证
> 产品定位：轻量、离线优先的桌面录屏工具

## 1. 核心目标

拾屏服务于会议演示、教程录制、问题复现和临时分享。核心流程必须保持为：选择目标、确认声音、开始录制、停止并获得本地文件。

产品不要求登录，不依赖云端，不内置复杂剪辑器，不在首版加入直播和内容管理。

## 2. 首版功能范围

| 能力 | 状态 | 产品说明 |
| --- | --- | --- |
| 整屏录制 | 已实现 | 默认入口，录制主显示器当前可见像素 |
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
| 全局快捷键与托盘 | 规划中 | 录屏主流程稳定后接入 |
| 异常恢复 | 规划中 | 尽量保留可恢复的未完成录制 |

“已实现”表示项目代码中已有直接实现路径，不等同于已经覆盖所有声卡、编码器、显示器和缩放组合。

## 3. 界面结构

主程序只保留一个紧凑的无标题栏圆角工具条窗口，录制前、录制中和暂停状态均在同一布局内切换：

1. 左侧固定显示录制状态和计时器，计时器下方显示当前来源、清晰度、帧率或简短状态。
2. 右侧依次提供来源、清晰度、帧率、系统声、麦克风和开始/停止。
3. 未录制时，“范围”按钮通过紧凑下拉明确选择全屏、窗口或区域；清晰度同样通过紧凑下拉选择自动、720p、1080p 或原始分辨率；帧率只有 30/60 FPS 两项，继续按点击循环。
4. 录制中，来源按钮切换为暂停/继续；来源、清晰度和帧率锁定，声音仍可独立开关。
5. 停止和保存结果继续显示在计时器下方，不打开完成弹窗。
6. 鼠标细项、倒计时、保存目录和固定 MP4 格式不常驻占用工具条空间；保存目录与退出等低频操作放入原生右键菜单。
7. 无标题栏窗口的所有非交互空白区域均可拖动；空闲状态不重复显示“就绪”，只保留参数摘要和“开始”动作。

界面基于 Slint 1.17。已核对原生 `Button`、`Switch`、`ComboBox`、`Palette`、`TouchArea`、`FocusScope`、`PopupWindow`、`ContextMenuArea`、`Menu`、`MenuItem` 和 `MenuSeparator`，并检查了 `shi-ui` 现有的 `StatusBar`。原生 `Button` 的公开结构固定为横向图标与文字，不能表达参考设计所需的“圆形图标 + 下方标签 + 紧凑选中态”；原生 `ComboBox` 的 Fluent 实现最小宽度为 160px，也不能放入 46×57px 的快捷操作位；`ContextMenuArea.show(Point)` 在当前无边框窗口经过系统拖动后无法稳定再次弹出；公共 `StatusBar` 的固定高度和状态结构不能容纳录制状态、计时与参数摘要。因此只为六个快捷操作和录制状态区自定义视觉结构。范围与清晰度使用 Win32 `CreatePopupMenu`、`AppendMenuW` 和 `TrackPopupMenu` 创建原生选择菜单，由系统处理键盘、失焦关闭、屏幕边缘与缩放；应用右键菜单继续使用 Slint 原生 `ContextMenuArea`、`Menu` 与 `MenuItem`，没有新增自定义弹层窗口。

## 4. 默认值

- 录制目标：整个屏幕。
- 清晰度：1080p。
- 帧率：30 FPS。
- 系统声音：开启。
- 麦克风：关闭。
- 显示鼠标：开启。
- 突出鼠标点击：关闭。
- 开始倒计时：3 秒。
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

## 6. 当前实现边界

- 当前仅实现 Windows 路径。画面通过 GDI 读取屏幕当前可见像素，因此窗口录制不是独立的窗口表面捕获：遮挡、屏幕外区域和窗口移动都会反映到录制结果中。
- “自动”与“1080p”当前都以 1080p 为上限；720p 和 1080p 均保持原始宽高比、不主动放大，并将编码尺寸调整为偶数。
- 视频通过 Media Foundation Sink Writer 写入 H.264/MP4，并使用 CBR 控制文件体积：720p30 为 2.5 Mbps、720p60 为 4 Mbps、1080p30 为 4 Mbps、1080p60 为 6 Mbps；原始分辨率按像素数量同比增加，30 FPS 上限 12 Mbps、60 FPS 上限 18 Mbps。
- 声音以 48 kHz、双声道、16 位 PCM 输入 Sink Writer，由系统编码为 192 kbps AAC。系统声音与麦克风同时开启时在应用内混音。
- 音频端点、媒体编码器或输出目录不可用时明确报错，不切换到其他采集源或生成伪文件。完成前使用 `.partial.mp4`，只有编码器正常结束才改名为最终文件。
- 输出目录、来源、清晰度、帧率、声音、鼠标和倒计时设置持久化到 `%APPDATA%\ShiPing\config.json`。
- 全局快捷键、托盘和异常恢复仍为规划中，首版没有备用实现路径。

## 7. 官方依据

- [Media Foundation：使用 Sink Writer 编码视频](https://learn.microsoft.com/en-us/windows/win32/medfound/tutorial--using-the-sink-writer-to-encode-video)
- [`MFCreateSinkWriterFromURL`](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-mfcreatesinkwriterfromurl) 与 [`IMFSinkWriter::SetInputMediaType`](https://learn.microsoft.com/en-us/windows/win32/api/mfreadwrite/nf-mfreadwrite-imfsinkwriter-setinputmediatype)
- [WASAPI 回环录制](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)
- [AAC 编码器](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-encoder) 与 [AAC 媒体类型](https://learn.microsoft.com/en-us/windows/win32/medfound/aac-media-types)

## 8. 验证状态

- 已通过：`cargo check -p shiping`、常规单元测试、短时真实 MP4 录制测试和 Debug 构建。
- 当前设备已验证：主界面与右键菜单、3 秒倒计时、录制/暂停/继续/停止、结果文件生成、Escape 与右键取消目标选择、可见窗口选择和区域拖选。
- 尚未覆盖：多显示器含负坐标、不同 DPI/缩放组合、60 FPS 持续负载、不同音频端点格式、无 AAC/H.264 编码器设备、长时间录制和发布构建安装包。编译通过不能替代这些设备矩阵验证。
