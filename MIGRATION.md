# ShiTu 目录整理与 GPUI 迁移清单

截至 2026-10-10，目录整理和 Slint → GPUI Kit 代码迁移已完成，Debug/Release 构建、63 项测试、严格 Clippy 和未签名 MSIX 打包通过。主设置窗口已做部分桌面实测；截图、标注、贴图的完整交互验收尚未完成。下方分别记录实现与实测，不以编译通过替代功能验收。

## 目录整理：已完成

- [x] 应用提升到根 Cargo 包 `shitu`，二进制名保持 `ShiTu`；合并原 shi-foundation、shi-ui，移除 apps/crates 分包。
- [x] 删除未实现的拾音占位应用、录音依赖和多产品启动入口。
- [x] 图标统一到 assets/icons，旧版参考图移到 assets/shitu_01.jpg；Store 清单移到 packaging/AppxManifest.xml。
- [x] 保留本地产品文档、已有发布产物及用户原有 CI 改动；未清理用户忽略文件和临时研究目录。
- [x] 保留 tools/windows-ai-bindgen 独立工具包：它用于生成 Windows AI 绑定，不属于应用拆包。

当前目录职责：

| 目录/文件 | 职责 |
| --- | --- |
| src/app.rs、src/app/ | 设置、截图/贴图编辑器、标注、工具栏布局、托盘 |
| src/platform/ | Windows 截图、OCR、剪贴板、窗口和启动注册适配 |
| src/*.rs | 配置事务、图像处理、输出、快捷键、语言和日志 |
| assets/ | 应用资源及旧版参考图 |
| translations/ | 十种语言 PO 文案 |
| packaging/ | Store 清单和打包说明 |
| tools/ | MSIX 打包、翻译编译、AI 绑定生成 |
| Cargo.toml、build.rs、start.sh | 单应用依赖、资源构建、启动入口 |

## UI 替换：代码已完成

- [x] GPUI Kit 0.7.1 入口、主题预览、托盘、全局快捷键分发。
- [x] 常规、截图、OCR、钉住、快捷键、关于设置页；配置事务保存、当前页恢复默认、目录选择、语言预览、异步模型准备。
- [x] 图像缓冲和标注数据脱离 Slint 类型；保留像素处理、撤销重做和业务测试。
- [x] 截图遮罩、窗口识别、选区、标注工具、复制/保存/OCR/钉住输出。
- [x] 贴图缩放、透明度、置顶、标注、浮动工具栏和原生右键菜单，与截图共用编辑器。
- [x] OCR 结果窗口、复制、异步任务成功/失败反馈。
- [x] 十种语言与构建期翻译完整性检查；Input/Textarea 原生编辑菜单使用项目译文。
- [x] 删除 Slint 源码、构建依赖、Skia 下载逻辑和 vendor 补丁；Cargo.lock 无 Slint 依赖。
- [x] 更新 README、AGENTS、构建和打包说明，检查旧源码路径。

## 官方依据与实现边界

- 对照 LanDesk `960015ad5006ce10dcb7acef968bda1cfcfb77a4` 及本机锁定依赖源码：gpui-kit/gpui-component/gpui-base 0.7.1，底层 gpui-pre 0.3.8；Rust 1.96、edition 2024。仅参考 UI 组织方式，不复制其 SSH 业务。
- 表单与菜单使用官方 Input、Textarea、Button、Checkbox、TabBar/Tab、DropdownMenu/PopupMenu、NativeMenu；业务图像使用 canvas 和 Window::paint_image。
- Windows 后端尚未实现 AnchoredPopup；NativeMenu 有独立 Windows 实现。项目使用原生菜单处理文本和贴图菜单，未仿制锚定菜单窗口。
- 截图选区、标注和浮动工具栏为业务专用交互。工具栏使用 GPUI PopUp 窗口；Windows 适配仅补充物理坐标定位、工作区、置顶和不激活显示等能力。失焦、边缘和多屏 DPI 行为仍须实测。
- 图像业务数据为 RGBA，提交 GPUI RenderImage 时转换成 BGRA。贴图关闭 Root 默认不透明背景，防止覆盖图片透明度；视觉效果仍待验收。
- GPUI UI 线程使用 OLE STA，系统 OCR 可用性探测放到独立 MTA 工作线程。系统 OCR 探测成功不代表实际识别已经验证。
- 工具栏通过快照渲染，避免渲染时重入读取正在更新的编辑器；文本菜单只构造译文与官方 actions，不重入读取 InputState。
- build.rs 只嵌入图标与版本资源；GPUI 提供进程 manifest，已从 EXE 提取核对 PerMonitorV2 等声明，避免重复 MANIFEST 链接错误。Store 身份保留于 AppxManifest.xml；本项目未实现外部位置/稀疏包注册，删除了旧的独立身份 manifest。
- Store 清单仍要求 Windows build 26100 和 Windows App Runtime 1.8。Windows AI OCR 仍为实验能力，本机未验证受支持 NPU/模型环境。

官方资料：[GPUI Kit 0.7.1](https://docs.rs/gpui-kit/0.7.1/gpui_kit/)、[组件目录](https://gpui-kit.com/component/)、[Input::context_menu](https://docs.rs/gpui-component/0.7.1/gpui_component/input/struct.Input.html#method.context_menu)、[Windows 后端](https://docs.rs/crate/gpui-pre-windows/0.3.8/source/src/window.rs)、[Cargo 包与工作区](https://doc.rust-lang.org/cargo/reference/workspaces.html)、[Microsoft 外部位置身份包](https://learn.microsoft.com/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps)。

## 当前环境验证

- [x] `cargo test --offline --locked`：72 项通过，覆盖图像/标注、配置、翻译、快捷键过滤、布局、OCR 转换、缩放、原始像素预览、配置回滚错误、原生工具提示控件、笔刷覆盖范围、滚轮数值调整、画笔/箭头虚线、字体属性保存和字号单位换算。
- [x] `cargo clippy --offline --locked --all-targets -- -D warnings`。
- [x] `cargo build --offline --locked` 与 `cargo build --release --offline --locked`。
- [x] Rustfmt、UTF-8、diff 检查，Bash 与打包 PowerShell 脚本语法检查。
- [x] MakeAppx 打包成功，生成未签名 MSIX 与 msixupload；未安装、签名或上传 Store。
- [x] 最终窗口显示顺序修正后再次通过 Rustfmt、严格 Clippy 和 Release 构建；使用最终 EXE 重新打包成功，并启动 Release 确认主窗口显示“就绪”。
- [x] Debug 手动启动主窗口；常规/截图/OCR 标签切换、深色主题预览、英语即时预览、十种语言菜单显示。
- [x] 输入框选择和编辑；在截图页输入无效草稿后，切到 OCR 页恢复默认，再切回仍保留草稿。
- [x] 最新 Debug 输入框右键显示中文原生编辑菜单，Esc 关闭后 Ctrl+A 仍能选中文字；未复现实体重入崩溃。
- [x] 系统 OCR 可用性探测显示可用；AI 不可用状态与禁用操作显示。
- [x] 开始截图可完成屏幕抓取并创建截图窗口，进程继续运行；不等同于选区/标注/输出实测。

实测未保存设置，保留用户配置。经用户授权退出工作区旧版应用后，已成功注册并触发 Ctrl+Alt+C；最新 Debug 在窗口激活后记录的画布为 3840×1132，与原图一致。

## 2026-10-10 关于页与工具条修正

- 恢复关于页的应用图标、名称、版本、构建环境/日期、作者、Twitter/GitHub 链接和版权信息，隐藏该页无关的保存/恢复默认按钮。已在实际设置窗口核对布局。
- 工具条使用分组图标按钮、选中/禁用状态、独立色块、实线/虚线按钮和完整字号选项。默认收起属性，仅展开当前工具需要的颜色、线宽、字号或马赛克大小；定位预留展开空间。
- 使用同一工具条组件的临时普通窗口预览，检查浅色/深色下的收起、矩形、文字和马赛克布局，修正数字截断；预览代码已移除。这是组件视觉验证，不等于截图浮窗的完整操作验收。

## 待完成的桌面验收

### 关于页技术说明与工具条浮层

- 按用户要求移除关于页的平台/架构、GPUI Kit、构建类型/日期说明，以及底部“使用 Rust 与 GPUI Kit 构建”整行。保留图标、应用名称、版本、简介、作者和链接；此项是界面信息调整，不代表提供防逆向保护。
- 字号、线宽、马赛克大小、实线/虚线下拉改用 GPUI Component 0.7.1 的 `NativeMenu`，由系统显示窗口外菜单和处理关闭。移除工具条旧 `DropdownMenu/PopupMenuItem` 路径；自定义字号输入仍使用 `NumberInput`。
- GPUI Base 0.7.1 的 `TooltipOverlay` 在窗口内渲染，首次延迟 500ms，存在 300ms 连续切换宽限期；Windows 0.3.8 后端仍未实现 `AnchoredPopup`。因此工具条提示改用 Windows 标准 `TOOLTIPS_CLASSW` 控件，`TTF_SUBCLASS` 自动接收按钮区域鼠标消息，`TTM_SETDELAYTIME` 将首次/切换延迟统一为 600ms，显示 5 秒后自动关闭。不手写悬停计时器、定位或失焦策略。
- 提示区域来自每帧实际布局并按 DPI 转成物理客户区坐标；仅在区域/文本改变时更新，移除不再显示的按钮，保持原生计时器稳定。控件由工具条持有并在关闭时释放，不激活窗口。
- 已核对 Button、Tooltip/TooltipOverlay、DropdownMenu、NativeMenu、NumberInput 官方源码及 windows 0.62.2 绑定。64 项测试与严格 Clippy 通过，新增隐藏原生控件测试验证首次/切换延迟、区域移动、旧区域删除、同位置文字替换和控件销毁。没有桌面鼠标/键盘自动验收；真实外观、菜单选择、不同 DPI 与边缘避让由用户测试。
- Release 构建、格式/UTF-8/diff 检查通过。已按用户授权替换并重启 `D:\tool\ShiTu.exe`，替换后 SHA-256 与工作区 Release 一致；未提交或推送。

官方资料：[NativeMenu 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/native_menu/struct.NativeMenu.html)、[Windows Tooltip 控件](https://learn.microsoft.com/en-us/windows/win32/controls/tooltip-controls)、[TTM_SETDELAYTIME](https://learn.microsoft.com/en-us/windows/win32/controls/ttm-setdelaytime)、[TTTOOLINFOW](https://learn.microsoft.com/en-us/windows/win32/api/commctrl/ns-commctrl-tttoolinfow)。

### 截图保留主窗口显示状态

- 快捷键、托盘截图和“开始截图”按钮均不再隐藏主窗口；截图结束或失败时也不主动恢复/激活它。显示中的拾图窗口可以包含在截图内，已经收进托盘的窗口继续保持隐藏。
- 删除截图专用的 `restore_main` 状态和不再使用的可见性查询函数。保留现有 160ms 延迟，供触发截图的托盘菜单关闭，不新增配置项或平台 API。
- 63 项测试、严格 Clippy、格式/UTF-8/diff 检查和 Release 构建通过；工作区构建不会自动替换 `D:\tool\ShiTu.exe`。
- 此项需要复验：主窗口显示时截图自身、主窗口已隐藏时截图、取消/完成/失败后的窗口状态。

### 2026-10-10 工具条遮挡与贴图交互回归

- 工具条没有设置 owner，截图画布重新激活后，同为置顶窗口的画布可以盖住工具条。已通过 `SetWindowLongPtrW(GWLP_HWNDPARENT)` 将每个工具条绑定到自己的编辑器，使用 Windows 的 owned-window 层级规则。GPUI 0.3.8 的 `WindowOptions` 没有 owner 参数，Windows 后端只有模态 Dialog 设置 owner；因此只在现有 Windows 适配层补此项，不新增窗口或抢焦点循环。
- 贴图原先调用 `Window::start_window_move()`，但 gpui-pre-windows 0.3.8 未实现该方法，实际走空默认实现。已改为官方 TitleBar 使用的 `InteractiveElement::window_control_area(WindowControlArea::Drag)`，后端将该区域映射成 `HTCAPTION`。仅在未选择标注工具、且不忙时启用；双击事件自行处理关闭设置，右键事件交给原生业务菜单，阻止 Windows 额外处理标题栏菜单/最大化。
- 补回旧版贴图的 1px `#54a5ff` 蓝色轮廓，使用 GPUI 官方 `outline` 绘制，仅影响窗口显示，不改动导出图片像素。
- 已核对组件/API：GPUI Kit/gpui-component 0.7.1 的 TitleBar、Button、NativeMenu；gpui-pre/gpui-pre-windows 0.3.8 的窗口控制区域、非客户区鼠标分发、WindowOptions 和 outline；windows 0.62.2 的窗口所有者绑定。
- 本轮 63 项测试、严格 Clippy、格式/UTF-8/diff 检查、Debug/Release 构建与重新打包通过；已更新并启动 Release。这些检查不证明真实浮窗拖动和层级行为已经验收；工具条在画笔操作后的可见性、贴图拖动/跟随、双击和右键仍列为桌面验收项。

官方依据：[GPUI Windows 0.3.8 鼠标分发与命中测试](https://docs.rs/crate/gpui-pre-windows/0.3.8/source/src/events.rs)、[GPUI Component 0.7.1 TitleBar](https://docs.rs/crate/gpui-component/0.7.1/source/src/title_bar.rs)、[SetWindowLongPtrW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw)、[Windows owned-window 层级规则](https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features#owned-windows)。

本轮截图回归修复：

- 原因已复现：3840×1132 原图被绘制到 3824×1124 客户区。GPUI Windows 0.3.8 的 PopUp 使用零样式（WS_OVERLAPPED），Windows 添加的非客户区边框使内容区变小。设置真实 WS_POPUP 并移除框架样式后，日志确认画布物理尺寸为 3840×1132，与原图一致；截图绘制也改成原始像素映射，不再拉伸适配窗口。新增 100%/125%/150%/200% 映射回归测试。
- 后端给所有窗口设置 NonRudeHWND，允许任务栏覆盖。仅在截图窗口移除这一由 GPUI 添加的属性，并显式置顶、激活；贴图和工具条保留非全屏任务栏策略。未使用 GPUI 的单显示器 fullscreen，因为本项目截图覆盖多显示器虚拟桌面。
- 截图、贴图、工具条在创建后立即隐藏，完成窗口配置后使用 SW_SHOWNA 显示，保留当前物理尺寸。未使用 WindowOptions::show=false：GPUI 0.3.8 会延迟 initial_placement，首次激活时重新应用默认尺寸；此问题已复现并修正，最新快捷键测试中最终画布保持 3840×1132。
- 工具条再次按旧版源码恢复：462px 宽、38px 主栏、106px 展开区、28px 按钮/16px 图标、蓝色选中边框、两行六色色板和右侧 188px 参数控件。恢复实际线宽 2–24、马赛克 20/36/60、字号预设及 8–96 自定义输入；去掉底部状态条，选区尺寸显示在选区边缘。
- 已用真实工具条组件的临时普通窗口核对上述布局，临时入口已移除。窗口尺寸日志确认不是任务栏工作区导致裁切；任务栏覆盖、多屏 DPI 和完整截图操作仍需桌面实测，不以属性设置成功替代视觉验收。

对应源码/API：[GPUI Windows 0.3.8 window.rs](https://docs.rs/crate/gpui-pre-windows/0.3.8/source/src/window.rs)、[SetWindowLongPtrW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw)、[RemovePropW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-removepropw)、[ShowWindow](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow)。

当前 Computer Use 能定位主设置窗口，但不能枚举截图/工具栏/贴图的 PopUp 工具窗口，因此未使用猜测句柄继续操作。以下项目保留未勾选，需要能操作这些窗口的桌面环境验收：

- [ ] 截图拖选、窗口吸附、选区移动/缩放、Esc 取消，以及复制、保存、自动保存、钉住的完整流程。
- [ ] 画笔、矩形、椭圆、箭头、文字、橡皮擦、马赛克；中文 IME、选择/移动/缩放、撤销重做。
- [ ] 贴图缩放、透明度、置顶、替换图片、工具栏及原生菜单的完整操作。
- [ ] 截图/贴图的键盘、失焦关闭、屏幕边缘、多屏和不同缩放比例；窗口阴影与透明效果。
- [ ] 对实际截图完成系统 OCR，核对结果编辑和复制；托盘完整操作。
- [ ] Store 安装/启动和受支持设备上的 Windows AI OCR。其他 Windows 版本/GPU 驱动尚未验证。

### 笔刷大小与滚轮调整

- 已检查 gpui-component/gpui-base 0.7.1 的 NumberInput、InputState、NumberStep、NativeMenu，以及 gpui-pre 0.3.8 的 on_scroll_wheel、on_hover 和 canvas。直接复用数字输入、加减、范围校验及原生预设菜单；NumberInput 没有滚轮调整 API，因此只在数值控件外组合 GPUI 滚轮事件，不新增窗口或平台接口。
- 马赛克、橡皮擦仅保留左侧圆点和右侧一个带加减按钮的尺寸输入框，参数区压缩到 40px；字号也仅保留一个输入框。笔刷直径 4–128px，按 2px 步进（底层为整数像素半径），奇数提交时向上取偶数。预览槽固定，圆点随数值缩放；画布上的双色轮廓按实际图像像素及贴图缩放绘制，不写入导出图像。
- 画笔、矩形、圆形、箭头保留线宽原生下拉，右侧固定 32px 预览槽显示相应直径的圆点；浅色主题为黑点、深色主题为浅色点。四类工具下方统一使用官方 RadioGroup/Radio 选择实线和虚线，以线段样式作标签并提供翻译后的可访问名称，保留两行色板；通常参数区为 64px。已删除旧线型菜单及其 Action 分支。
- 已核对 gpui-component 0.7.1 的 Input::render（StyleRefinement 在默认字号和高度之后应用）和 RadioGroup::horizontal/selected_index/on_change、Radio 的指针/键盘交互实现。编辑中的文字使用所选字号乘以图像显示比例，默认字体为 Microsoft YaHei UI，颜色跟随所选颜色；宽度限制在图片剩余区域，使用原生输入框的横向滚动与 IME。字号右侧显示“字”样，以所选图像像素字号除以工具条 DPI 比例显示；大字号时参数区按需增高，并重新计算边缘避让。样字展示 100% 图像比例，贴图缩放不改变字号值。
- 画笔虚线按整条采样路径计算连续的虚线相位，箭头采用虚线箭杆和实线箭头。复用既有图形虚线算法，预览、导出与擦除命中使用同一段线几何；新增测试验证采样点变化不重置虚线、间隙不被错误擦除、撤销重做恢复绘制结果。输入中字号、Radio 键盘操作和屏幕边缘的实际视觉效果仍待用户桌面测试。
- 同一个编辑窗口内，各工具分别记住大小；不新增磁盘配置。马赛克颗粒固定为原默认的 10px，大小仅影响覆盖范围。橡皮擦保留整条标注删除语义，按选定半径持续命中，补齐鼠标采样之间的路径，整次擦除可一次撤销。
- 字号、线宽、笔刷数字区域支持悬停滚轮微调，Shift 加速 5 倍，达到上下限停止。Windows 0.3.8 会将 Shift+竖向滚轮映射到 X 轴，已按官方后端实现处理；其他区域保留原交互。
- 自动测试覆盖擦除半径/快速拖动/撤销重做、马赛克覆盖与颗粒分离、100%/125%/150%/200% DPI 和贴图缩放下的范围映射、滚轮轴向及数值上下限。按用户要求不操作桌面；鼠标/键盘、输入失焦、原生菜单边缘避让及实际多屏 DPI 视觉效果仍由用户测试。

官方依据：[NumberInput 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/input/struct.NumberInput.html)、[InputState 0.7.1](https://docs.rs/gpui-base/0.7.1/gpui_base/input/struct.InputState.html)、[Windows 0.3.8 滚轮事件](https://docs.rs/crate/gpui-pre-windows/0.3.8/source/src/events.rs)。

本轮组件依据：[RadioGroup 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/radio/struct.RadioGroup.html)、[Input 0.7.1 实现](https://docs.rs/crate/gpui-component/0.7.1/source/src/input/input.rs)。

### Windows 系统字体设置

- 文字参数区增加“字体设置…”按钮，调用 Windows ChooseFontW。已检查 GPUI Kit 0.7.1 的 Button/Input/NumberInput 和 Windows 后端，未找到系统字体对话框封装；在现有 Windows 适配层使用 windows 0.62.2 的 CHOOSEFONTW/LOGFONTW，不自绘字体列表或弹窗。
- 系统对话框管理字体、字形和字号选择；应用接收字体名称、字重和斜体状态。颜色继续由色板选择，未启用 CF_EFFECTS，第一版不提供下划线/删除线。仅列出横排字体，不显示脚本选择；取消不修改任何字体属性，失败通过 CommDlgExtendedError 明确报错。
- 系统显示 6–72pt，工具条显示 8–96px，固定按 96px = 72pt 换算并取整到图像像素。初始化 LOGFONT 高度时使用屏幕 DC 的 LOGPIXELSY，以抵消系统对话框的 DPI 换算；显示缩放不改变导出字号。
- 对话框使用独立线程和编辑器 owner，避免原生模态循环重入借用中的 GPUI 视图。期间禁用该编辑器工具条及编辑操作；系统返回后通过命令队列应用结果，恢复原有输入焦点。没有修改主程序配置或新增跨平台路径。
- 新选择作用于正在输入及之后创建的文字。每条已提交文字独立保存字体名称、字重、斜体和字号；撤销/重做、移动/缩放保留这些属性，导出使用 CreateFontIndirectW。输入框和样字使用相同字体名称/字重/斜体。已有文字不会被后续全局字体选择批量改写；文本选择边框仍沿用现有估算方式，未在本轮改为精确字体度量。
- 自动验证包含字号换算、LOGFONT 字段、导出字重/斜体产生不同像素以及独立标注属性的撤销和缩放。系统面板打开/确认/取消、字体预览、焦点恢复、不同 DPI 下单位显示和多屏层级仍待用户手工验收，未自动操作桌面。

官方依据：[Font dialog box](https://learn.microsoft.com/en-us/windows/win32/dlgbox/font-dialog-box)、[CHOOSEFONTW](https://learn.microsoft.com/en-us/windows/win32/api/commdlg/ns-commdlg-choosefontw)、[ChooseFontW](https://learn.microsoft.com/en-us/windows/win32/api/commdlg/nf-commdlg-choosefontw)、[CreateFontIndirectW](https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createfontindirectw)。

### 文字面板与快捷键布局细节

- 复用 GPUI Kit 0.7.1 的 NumberInput、InputState 和 Button；字号输入与字体设置按钮均为 148px 宽，纵向排列，右侧 104px 的样字区跨两行居中。大字号仍按实际显示尺寸增加面板高度并重新计算边缘避让，没有新增窗口或焦点实现。
- 快捷键行统一为名称、输入框、清除按钮，删除默认值说明。留空显示“未设置”，清除按钮禁用；输入变化会刷新按钮状态，清除及保存行为保持原有语义。
- 工具条提示区域或标签变化时先发送 TTM_POP，移除可能残留的已显示提示；布局不变时不干扰系统悬停计时。初次与按钮切换仍为 600ms。隐藏窗口测试验证区域移动后旧位置不命中、新位置命中；截图所示提示出现位置是否仍有异常，需要用户实际鼠标操作复验。
- 72 项自动测试通过；不操作桌面，实际文字布局、键盘/鼠标、提示延迟、屏幕边缘及多种缩放比例仍待用户验收。

官方依据：[InputState 0.7.1](https://docs.rs/gpui-base/0.7.1/gpui_base/input/struct.InputState.html)、[Button 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/button/struct.Button.html)、[TTM_POP](https://learn.microsoft.com/en-us/windows/win32/controls/ttm-pop)。

### 快捷键按键录入

- 已检查 GPUI Kit/gpui-component/gpui-base 0.7.1 的 Input、InputState::set_readonly、InputEvent::Focus/Blur，以及 GPUI 0.3.8 的 capture_key_down、KeyDownEvent、KeybindingKeystroke::new_with_mapper 和窗口激活通知。组件没有现成快捷键录入器，复用官方只读输入框、键盘映射和焦点事件，只补项目组合键转换与待保存状态，不新增窗口或 Win32 键盘钩子。
- 输入框获得焦点后提示按键，完整组合直接显示；修饰键本身不提交。使用官方 Windows 键盘映射恢复 Shift+数字的实际组合。支持范围保持为修饰键加字母、数字、Space 或 F1–F12，不支持的组合明确提示无效。Esc 恢复本次录入前的值；Enter 结束录入但不保存，Tab/点击外部/窗口失活结束录入并保留完整的待保存组合；未录入完整组合时恢复原值。
- 录入、清除、恢复默认均不立即更改已注册快捷键或配置文件，只有点击保存才执行现有 settings::apply_transaction。保存失败保留待保存值并报告错误，沿用现有回滚处理。
- 录入期间不触发本软件截图；global-hotkey 0.8.0 发来的当前已注册组合事件用于填入录入框，不注销再注册。注册时比较解析后的组合，避免大小写或顺序变化导致同一组合重复注册。
- 74 项测试通过，新增覆盖录入值与注册解析的一致性、Shift 数字映射结果、无效组合、等价组合不重复注册及失败保留旧绑定。真实键盘录入、系统快捷键冲突、失焦切换及保存后实际触发仍需用户手工测试；系统保留组合是否能被应用接收不作保证。

官方依据：[InputState 0.7.1](https://docs.rs/gpui-base/0.7.1/gpui_base/input/struct.InputState.html)、[GPUI 0.3.8 键盘事件](https://docs.rs/gpui-pre/0.3.8/gpui/trait.InteractiveElement.html#method.capture_key_down)、[GPUI 0.3.8 键盘映射](https://docs.rs/gpui-pre/0.3.8/gpui/struct.KeybindingKeystroke.html)、[global-hotkey 0.8.0 Windows 实现](https://docs.rs/crate/global-hotkey/0.8.0/source/src/platform_impl/windows/mod.rs)。

## 本地运行与复验

运行最新 Release：`D:\workspace\12.aiwork\ShiTu\target\release\ShiTu.exe`。本轮已获准退出并更新工作区旧版应用；不要同时运行多个版本，以免占用快捷键。

```powershell
cargo fmt --all -- --check
cargo test --offline --locked
cargo clippy --offline --locked --all-targets -- -D warnings
cargo build --release --offline --locked
.\tools\package-store-msix.ps1 -Product ShiTu -ExecutablePath .\target\release\ShiTu.exe -Version 0.3.0 -OutputDirectory .\.codex-tmp\gpui-package-check
```

本次打包检查产物位于 `.codex-tmp/gpui-package-check/`，没有覆盖 release-assets 中的既有产物。未签名 MSIX 仅用于本次打包验证，不代表 Store 审核或侧载安装通过。
