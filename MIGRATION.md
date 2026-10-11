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

### 图库实现清单

- [x] “截图”后增加图库标签，首次进入扩宽窗口；左侧目录树和右侧图片区采用官方 ResizablePanel，可拖动分隔线。图库隐藏设置页的保存/恢复默认。
- [x] 保存目录作为默认入口，可添加其他目录；默认仅浏览当前目录的 PNG/JPG/JPEG。“从图库移除”只移除额外入口，不移动或删除原文件。无法读取目录时显示错误，不切换来源。
- [x] 目录按展开层级读取，跳过 reparse point 防止目录联接循环。恢复当前目录及其祖先展开路径；目录、当前路径、视图和排序原子写入独立 gallery.json，不覆盖设置草稿。
- [x] 缩略图/列表切换、文件名搜索、修改时间/名称/类型/大小排序；列表包含尺寸、大小和本地修改时间。GPUI uniform_list 虚拟化，后台生成 320×240 内缩略图，限制解码分配及缓存数量。过时结果不覆盖当前选择。
- [x] 双击/Enter 在软件内预览，支持上一张/下一张、Esc 关闭；右键支持文件夹定位、复制、OCR、重命名、移入回收站。方向键选择、F2 重命名、Delete 弹确认框。重命名保留扩展名，拒绝路径字符及保留名称。
- [x] 单张图片从右侧拖到左侧目录，目标高亮，成功刷新。同名不覆盖，同目录不操作。文件操作在后台执行，期间阻止重复操作。截图保存完成后通知图库刷新。
- [x] 核对并复用 GPUI Kit/component/base 0.7.1 的 Tree/TreeState/TreeEvent、ResizablePanel、Input、Dialog/WindowExt、NativeMenu，以及 GPUI 0.3.8 的 uniform_list、on_drag/on_drop、drag_over 和键盘事件。只组合图片单元和文件业务，不自建拖放系统；独立预览窗口见下。
- [x] windows 0.62.2：MoveFileExW(COPY_ALLOWED) 不设 REPLACE_EXISTING；源文件删除未完成时回滚新副本并报错。IFileOperation 使用 RECYCLEONDELETE/ADDUNDORECORD 和系统不可回收警告，不实现永久删除兜底。文件时间使用 FileTimeToSystemTime/SystemTimeToTzSpecificLocalTime。
- [x] 77 项测试通过：目录非递归扫描、重名不覆盖、占用时保留源文件、实际移动内容校验、入口持久化、缩略图比例及损坏文件错误。
- [ ] 用户桌面验收：目录树、拖放释放目标、回收站恢复、菜单/对话框焦点、不同 DPI 和窄窗口、多磁盘/网络目录。按用户要求未操作桌面，编译通过不代表交互验收通过。

官方依据：[Tree 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/tree/index.html)、[ResizablePanel 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/resizable/index.html)、[GPUI 拖放](https://docs.rs/gpui-pre/0.3.8/gpui/trait.StatefulInteractiveElement.html#method.on_drag)、[IFileOperation 标志](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags)、[MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)、[文件时间转换](https://learn.microsoft.com/en-us/windows/win32/api/timezoneapi/nf-timezoneapi-systemtimetotzspecificlocaltime)。

### 图库界面精简（2026-10-10）

- [x] 移除公共“开始截图”操作行，截图继续由快捷键和托盘菜单触发；设置页的保存/恢复默认放在页面内容末尾。底部仅保留公共状态栏，图库状态同步到该区域。
- [x] 继续使用 Tree/ListItem 0.7.1，修正 ListItem 内容容器默认块布局造成的箭头与路径分行；使用横向组合、文件夹图标、34px 行高、主题侧栏底色和圆角选择态。路径单行省略，悬停展示完整路径。
- [x] 删除右侧路径/打开文件夹整行；搜索右侧依次为缩略图、列表、排序、刷新图标。排序复用 NativeMenu，时间由新到旧、名称升序、类型按扩展名升序、大小由大到小，同值按文件名排序；类型比较忽略扩展名大小写。打开文件夹保留在目录右键菜单。
- [x] 已检查 Tooltip 0.7.1、Button 的 managed tooltip 和 GPUI 0.3.8 的 StatefulInteractiveElement::tooltip_show_delay。Button 的 managed tooltip 固定 500ms 且短时间切换立即显示，不能满足每次延迟要求；图库改用 GPUI 原生 per-element tooltip 配合官方 Tooltip 内容，每个目标均设置 600ms，由框架处理离开取消、点击关闭和定位，无新增计时器或平台窗口。图库提示显示在主窗口顶层，不改动截图工具条已有的 Windows 原生提示。
- [x] 关于页的版本号独立为“当前版本”一行，使用正文大小和颜色。
- [ ] 桌面验收：目录省略/完整提示、快速划过与切换目标的 600ms 延迟、离开/滚动/切页后关闭、图标菜单、窗口缩放及不同 DPI。按用户要求不操作桌面。

组件依据：[ListItem 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/list/struct.ListItem.html)、[GPUI tooltip_show_delay 0.3.8](https://docs.rs/gpui-pre/0.3.8/gpui/trait.StatefulInteractiveElement.html#method.tooltip_show_delay)、[Tooltip 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/tooltip/struct.Tooltip.html)。对应锁定版本的本机源码已核对。

### 连续打开图片后停止加载与快捷键失效（2026-10-10）

- 现场进程两次采样：私有内存约 168MiB，未观察到持续增长或 OOM。用户确认连续双击图片调用系统看图程序后，标签仍可切换，但图库加载及截图快捷键停止；未操作桌面复现。
- 锁定 GPUI 0.3.8 的 AsyncApp::update_window 使用 AppCell::try_borrow_mut，临时重入会返回标准 BorrowMutError。原 Panel/Gallery/Editor 轮询对任何外层错误都直接退出，且未检查实体释放的内层结果。现仅对该明确的临时借用错误延后至下一次轮询，记录延后/恢复；窗口或实体释放时退出，其他错误记录后退出。测试验证临时冲突不会终止轮询。同步 ShellExecute 的 COM 消息重入是与症状一致的原因，但旧版没有相应日志，现场完整因果尚待用户复测。
- 打开目录和定位文件移到独立后台线程，按微软要求初始化 STA COM。单次只允许一个未完成的打开请求，不阻塞界面、截图轮询或目录扫描；失败回到公共状态栏。图片打开已由下述内部预览替代。
- 缩略图工作线程与目录/文件操作分离；请求队列最多 32 项、结果队列最多 16 项，切换目录后在解码前后检查代次并跳过旧任务。每轮界面最多接收 16 项结果。
- 缩略图改为后台直接生成 BGRA，再交给官方 RenderImage，不再 PNG 编码后由全局资产缓存二次解码。缓存按最近使用淘汰（当前上限见下）；刷新、淘汰、销毁时释放图像资源，调用官方 drop_image 移除 GPU 图集内容。
- Tree/ListItem 没有“仅截断时提示”属性：组合官方 StyledText::layout/TextLayout::text 与 canvas 读取实际省略后的排版，只有显示文字被截断才注册 Tooltip，仍延迟 600ms；无需按字符数猜测路径宽度。缩放和侧栏宽度变化会重新测量。
- 80 项测试通过，包括队列满时拒绝新增、跳过旧任务、坏图终态、BGRA 通道和轮询恢复；真实系统看图程序重复打开、快捷键恢复、长时间内存趋势及不同 DPI 的提示显示仍待用户验收。

官方依据：[GPUI AsyncApp 源码](https://docs.rs/crate/gpui-pre/0.3.8/source/src/app/async_context.rs)、[drop_image](https://docs.rs/gpui-pre/0.3.8/gpui/struct.App.html#method.drop_image)、[StyledText](https://docs.rs/gpui-pre/0.3.8/gpui/struct.StyledText.html#method.layout)、[ShellExecuteW 与 COM 初始化](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shellexecutew)。

### 内部预览与可见区域加载（2026-10-10）

- [x] 初版核对并复用 GPUI Kit/component/base 0.7.1 的 Dialog、DialogContent、Button、WindowExt，以及 GPUI 0.3.8 的 img/RenderImage、UniformListDecoration。当前预览已改为下述标准独立窗口，移除旧 Dialog 路径。
- [x] 双击、Enter、右键“图片预览”均在软件内打开，按当前搜索/排序结果切换上一张和下一张；等比适应预览区域。切图释放旧图 GPU 缓存，CPU 像素按下述缓存规则保留；关闭清空预览缓存，旧请求结果不会覆盖新选择。
- [x] 使用 UniformListDecoration::compute 提供的真实可见行范围调度缩略图，排除 uniform_list 为测量高度额外渲染的离屏行。滚出视口的任务在解码前后跳过；搜索无结果时清除旧视口任务。队列满时后续轮询继续补齐，不阻塞 UI。
- [x] 缩略图为 320×240 内的 BGRA，缓存上限为可见图片数与 64 的较大值，按最近使用淘汰。内部预览栅格限制在 2048×1536 内，保留原始尺寸信息，生成栅格时不放大小图；缓存上限见下。一个解码线程优先处理最新预览，避免多张原图并发解码。
- [x] image 0.25.10 的 ImageReader::limits 设置 max_alloc 为 128MiB；超限、损坏或读取失败显示错误。这不是进程总内存硬上限，原图仍需临时解码，不是分块读取。任务队列 32 项、缩略图结果队列 16 项、预览结果一个槽位。
- [x] 83 项测试、cargo check、严格 Clippy、格式/UTF-8/diff 检查和 Release 构建通过；新增覆盖真实行范围映射、预览取消/切换丢弃旧结果、预览优先、离屏任务跳过、预览缩放比例和不放大小图。已备份并替换 D:\tool\ShiTu.exe，核对 SHA-256 一致，重启进程存活且 Responding=True；这仅验证启动状态。
- [ ] 用户桌面验收：连续预览与关闭、上一张/下一张、Esc、滚动/搜索/缩放后加载、不同 DPI、预览期间截图快捷键。未自动操作桌面；用户报告上一版已显示缩略图、运行更流畅且所见内存约 20MB，不作为本轮大图预览的峰值保证。

官方依据：[Dialog 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/dialog/struct.Dialog.html)、[UniformListDecoration 0.3.8](https://docs.rs/gpui-pre/0.3.8/gpui/trait.UniformListDecoration.html)、[RenderImage 0.3.8](https://docs.rs/gpui-pre/0.3.8/gpui/struct.RenderImage.html)、[ImageReader 0.25.10](https://docs.rs/image/0.25.10/image/struct.ImageReader.html#method.limits)。已核对本机对应版本源码。

### 预览缓存与工具栏（2026-10-10）

- [x] 最近使用的预览像素最多 10 张且最多 100MB（100,000,000 字节），任一超限淘汰最久未使用项；命中会更新顺序并取消旧异步请求，不重复解码。同路径不会积累副本。刷新图库使缓存失效，关闭预览清空缓存。
- [x] 缓存保存 CPU 像素，切图调用官方 drop_image 移除上一张 GPU 图集记录；只有当前显示图上传 GPU。旋转直接读取缓存像素，生成当前角度的一份临时图，不缓存多个旋转版本，不改写文件。100MB 不包含解码中间数据、旋转临时图、缩略图、GPU 资源及进程其他内存，因此不代表整个进程上限。
- [x] 已检查并复用 GPUI Kit 0.7.1 的 Toolbar、Button、Icon、Dialog、DialogContent，GPUI 0.3.8 的 canvas/Window::paint_image 与原生鼠标事件；未发现完整图片查看器组件。自定义图片变换和缓存；当前窗口与焦点处理见下述标准独立窗口方案。
- [x] 图片两侧用左右箭头切图；图片区域获得焦点时键盘左右键切图，工具栏获得焦点时保留原生箭头键导航。底部工具栏含缩小、当前比例、放大、适应窗口、左右旋转 90°；图标提示复用现有每次 600ms 延迟。
- [x] 滚轮以鼠标位置为缩放中心，放大后按住左键拖动，位置限制确保图片不会被拖出可见范围。切图和旋转恢复适应窗口；比例按原图尺寸和显示 DPI 计算。放大仍使用有界预览栅格，不提供原图分块解码或额外高清加载。
- [x] 88 项测试和严格 Clippy 通过：500 次缓存插入/替换验证数量与字节上限、弱引用证明缓存淘汰与清空后像素对象释放；覆盖重复路径、超大项、最近访问顺序、1/1.25/1.5/2 倍 DPI 下的缩放和拖动边界。这些测试不证明 GPU 驱动内存或真实桌面长期趋势已经验收。
- [x] Release 构建及格式/UTF-8/diff 检查通过；备份并更新 D:\tool\ShiTu.exe，SHA-256 与构建产物一致，重启后进程存活且 Responding=True。仅验证启动，没有自动操作预览界面。
- [ ] 用户桌面验收：重复切图和关闭后的内存趋势、缓存命中、旋转、滚轮缩放与拖动、Esc/焦点恢复、不同 DPI 与窗口边缘。按用户要求不自动操作桌面。日志记录每次新图进入缓存的数量/像素字节，以及关闭清空事件，供持续增长时定位。

官方依据：[Toolbar 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/toolbar/struct.Toolbar.html)、[Window::paint_image 0.3.8](https://docs.rs/gpui-pre/0.3.8/gpui/struct.Window.html#method.paint_image)、[drop_image 0.3.8](https://docs.rs/gpui-pre/0.3.8/gpui/struct.App.html#method.drop_image)、[image::imageops 0.25.10](https://docs.rs/image/0.25.10/image/imageops/index.html)。已检查锁定版本本机源码。

### 图库多选与批量回收站（2026-10-10）

- [x] 根因：普通 Dialog 0.7.1 的 button_props 只保存按钮属性，不自动构造 footer。原回收站确认框有 on_ok 回调，却没有可点击的确认/取消按钮。改用官方 AlertDialog.confirm()，由组件构造按钮、处理确认/取消、键盘和焦点恢复；一并修正图库重命名框的相同问题。
- [x] 已核对 GPUI Kit/component/base 0.7.1 的 List/ListState、ListItem、NativeMenu、AlertDialog、DialogFooter。ListState 仅管理一个 selected_index，现有缩略图网格使用 uniform_list，因此只新增 PictureSelection 状态管理 Ctrl 切换、Shift 固定锚点区间、Ctrl+Shift 合并区间；按当前搜索/排序结果计算，不自建输入、菜单或窗口系统。
- [x] 单击替换选择，Ctrl 点击追加/取消，Shift 点击选择连续区间；缩略图和列表共享状态，底部显示已选数量。右键已选项保留整组；右键未选项切为单选。多选时原生菜单保留“移入回收站”和下述“删除”，Delete 按键仍将选中组移入回收站；确认框显示总数。多选期间不提供未实现的批量拖动/重命名/复制；单选原有操作保留。目录/搜索变化清理不可见选择，Ctrl/Shift 点击不误开预览。
- [x] windows 0.62.2 IFileOperation：全组选项预检后逐项 DeleteItem 排队，一次 PerformOperations 执行；保留 RECYCLEONDELETE/ADDUNDORECORD/WANTNUKEWARNING，不添加永久删除兜底。GetAnyOperationsAborted 和源路径状态用于区分已处理与剩余；系统批量操作不是事务，部分失败明确报告数量、文件名与错误，重新扫描并保留剩余项选择，结果不会被刷新后的图片数量提示覆盖。
- [x] 92 项常规测试、严格 Clippy 通过；另显式运行 1 项真实 Windows 回收站测试：只处理 .codex-tmp 下新建 PNG，覆盖两张批量成功，以及文件占用时准确报告剩余项、解除占用后重试成功。未操作用户图库文件，未执行系统回收站恢复测试。
- [x] Release、格式/UTF-8/diff 检查通过；备份并替换 D:\tool\ShiTu.exe，校验构建与部署 SHA-256 相同，启动后进程存活且 Responding=True。
- [ ] 桌面验收：Ctrl/Shift 实际鼠标选择、右键菜单、确认/取消与 Esc、不同缩放比例、批量回收后列表刷新及回收站恢复。按用户要求不自动操作桌面。

官方依据：[AlertDialog 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/dialog/struct.AlertDialog.html)、[IFileOperation::DeleteItem](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-deleteitem)、[PerformOperations](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-performoperations)、[SetOperationFlags](https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags)。已核对锁定版本源码。

### 图库细节与原生预览窗口（2026-10-10）

- [x] 回收站确认按钮改为“确定”，标题保留操作含义。缩略图卡片高度从 154px 调整至 164px，文件名保留 26px 高度、24px 行高且不压缩，长名称继续省略。移除图库右键“钉住”及对应后台任务；截图工具条的钉住功能保留。
- [x] 核对 GPUI Kit 0.7.1 的 Dialog、AlertDialog、NativeMenu、Toolbar 和 open_window，以及 GPUI/pre-windows 0.3.8 的 WindowOptions、TitlebarOptions、on_window_should_close。Dialog 是窗口内弹层，不能提供系统标题栏；采用 open_window 的标准 Normal 窗口，保留默认标题栏，is_resizable/is_minimizable 为 true。Windows 后端直接启用 WS_MAXIMIZEBOX/WS_MINIMIZEBOX，没有自写 Win32 窗口控制。
- [x] 单个预览窗口复用，支持原生最大化/还原、最小化与关闭，内容随窗口布局。Esc 和系统关闭均取消预览请求、清空缓存。主窗口可继续使用；上一张/下一张固定采用本次打开时的列表顺序，不受主窗口后续选择或排序干扰。再次从图库打开图片会更新此顺序。
- [x] 92 项常规测试及严格 Clippy 通过；回收站真实系统测试本轮未重复运行。缓存、缩放、DPI 几何、多选和异步取消逻辑测试通过。
- [x] Release 构建、格式/UTF-8/diff 检查通过。已备份并替换 D:\tool\ShiTu.exe，构建与部署 SHA-256 均为 3379955EADE158AB06270049C675A9BA527B5C15122C993CC8C4F44316594B19；启动进程 39992 存活且 Responding=True，仅作为启动验证。
- [ ] 桌面验收：最大化/还原、最小化后再打开、Esc/系统关闭后重开、窗口边缘与不同 DPI、文件名显示。未自动操作用户桌面；编译和逻辑测试不等于窗口交互验证。

官方依据：[open_window 0.7.1](https://docs.rs/gpui-kit/0.7.1/gpui_kit/fn.open_window.html)、[WindowOptions 0.3.8](https://docs.rs/gpui-pre/0.3.8/gpui/struct.WindowOptions.html)、[TitlebarOptions 0.3.8](https://docs.rs/gpui-pre/0.3.8/gpui/struct.TitlebarOptions.html)。已核对本机锁定版本源码。

### 图库永久删除（2026-10-10）

- [x] 右键新增“删除”，与“移入回收站”并列，单选/多选均可使用。确认框标题为“永久删除”，列出文件名或图片数量，明确提示“将永久删除所选图片，不会移入回收站，无法从回收站恢复。”；按钮为“确定／取消”。键盘 Delete 仍执行移入回收站。
- [x] 复用已核对的 GPUI Kit/component 0.7.1 NativeMenu、AlertDialog、DialogButtonProps；没有新建窗口或焦点处理。永久删除使用本机 Rust 1.96.0 官方文档确认的 std::fs::remove_file，只删除选中的文件，不递归删除目录。与 IFileOperation 回收路径共用预检和结果结构，不作互相回退。
- [x] 全组选项预检和去重后在后台删除，遇到首个错误停止，显示已删除数量、剩余项及错误；刷新列表并保留剩余选择。永久删除不具备事务回滚。十种语言文案已同步。
- [x] 95 项测试、严格 Clippy 通过。新增 3 项真实临时文件测试：只删选中项/去重、缺失文件或目录/非图片预检、共享占用导致部分失败及解锁重试；未删除用户图库文件。真实回收站测试本轮保持忽略。
- [x] Release、格式/UTF-8/diff 检查通过；备份并替换 D:\tool\ShiTu.exe，SHA-256 与构建产物一致，启动进程 12828 存活且 Responding=True。此项只验证部署及启动。
- [ ] 桌面验收：单选/多选右键菜单、确认/取消/Esc、长文件名与不同缩放比例。未自动操作用户桌面。

官方依据：[AlertDialog 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/dialog/struct.AlertDialog.html)、[std::fs::remove_file](https://doc.rust-lang.org/1.96.0/std/fs/fn.remove_file.html)。已核对本机对应版本源码与离线标准库文档。

### 图库目录选中状态（2026-10-10）

- [x] 已核对 GPUI Kit/component 0.7.1 的 Tree、ListItem：ListItem 在自定义样式之后应用默认选中底色，不能仅通过 bg 覆盖。保留 Tree 的选中、键盘、展开、拖放行为，在选中行组合绝对定位的装饰层，使用主题蓝色 18%（浅色）/28%（深色）底色和 70% 蓝色细边框，文件夹图标使用主题蓝色；内容绘制在装饰层上方，不增加布局宽度或事件处理。
- [x] Release 构建、格式/UTF-8/diff 检查通过；备份并替换 D:\tool\ShiTu.exe，部署与构建 SHA-256 一致，启动进程 10308 存活且 Responding=True。此轮仅样式调整，未重复运行行为测试。
- [ ] 桌面验收：浅色/深色主题、鼠标与键盘切换目录、拖入文件夹时的选中及悬停效果。未自动操作桌面。

官方依据：[ListItem 0.7.1](https://docs.rs/gpui-component/0.7.1/gpui_component/list/struct.ListItem.html)，已核对本机对应版本 Tree/ListItem 渲染源码。

### 关于页检查与手动更新（2026-10-11）

- [x] 进入“关于”页自动在后台查询正式发布，不进行额外联网探测。自动检查失败静默结束，不显示错误或弹窗；主动检查的进度、暂无可用更新、失败原因，以及下载进度和更新结果统一显示在底部公共状态栏。检查并发受状态约束，单次请求总超时 20 秒。
- [x] 新版说明与“更新并重启”按钮直接显示在页面；只有点击按钮后才下载，不另弹确认框。页面预先提示保存未完成内容。复用 GPUI Kit/component 0.7.1 的 TabBar、Button 和滚动内容，未自建窗口或通知机制。
- [x] 锁定 ureq 2.12.1、semver 1.0.28、sha2 0.10.9、zip 4.6.1，已检查对应本机官方源码；使用 HTTPS、超时、响应大小限制和后台流式下载。只接受本仓库稳定版本、匹配的 x86_64 ZIP/校验文件，严格比较版本，禁止降级；下载后核验 SHA-256，ZIP 必须且只能包含根目录 ShiTu.exe。SHA-256 是完整性校验，不是独立发布签名。
- [x] Windows 0.62.2 的 GetCurrentPackageFullName 区分便携/打包进程，打包版本提示通过原分发渠道更新，不覆盖安装目录。便携版写入安装目录内独立 `.shitu-update-*` 目录，无写权限时在退出前明确失败。当前仅支持 Windows x86_64。
- [x] 复制当前程序作为更新辅助进程，握手确认就绪后主程序才退出。辅助进程通过 OpenProcess/WaitForSingleObject 等待确切进程退出，未提交或等待超时不替换文件；校验原程序和新程序后备份、替换、启动新版，并等待主窗口创建成功的启动回执。失败尝试恢复原程序；恢复失败保留备份并记录明确错误，不宣称更新成功。
- [x] 更新和回滚都不弹窗；结果由公共状态栏展示。辅助进程错误记录在配置目录 `logs/update.log`，替换后错误另记录在更新目录 `error.txt`。成功更新保留更新目录中的 `old.exe` 和辅助程序用于恢复，不自动清理备份目录；配置和图库目录引用不变。
- [x] 103 项常规测试、严格 Clippy 通过，涵盖静默/手动错误状态、版本比较、缺少或恶意附件、大小上限、校验文件绑定、ZIP 路径/多文件拒绝、真实临时文件替换/回滚与共享占用失败。
- [x] 单独执行真实联网测试，实际查询最新正式发布为 v0.3.0，本机 v0.4.0 不显示升级；成功下载其 8,413,118 字节 ZIP，核对发布 SHA-256、解压指定文件并经 GetBinaryTypeW 检查 Windows 64 位程序。未执行或安装下载的旧版。
- [x] `python tools/test-updater-helper.py` 使用实际 Release 辅助进程和独立 APPDATA、临时无窗口夹具，验证成功替换/启动回执、启动失败后恢复并重启旧程序、未提交保护、校验失败保护四种情况；未操作用户安装、配置或图片。这不等于真实新版本的桌面升级验收。
- [x] 最终 Release、格式/UTF-8/diff 检查通过；辅助进程四项测试在最终二进制上复验通过。已备份并替换 D:\tool\ShiTu.exe，构建与部署 SHA-256 一致，启动进程 24220 存活且 Responding=True，仅作为部署与启动验证。
- [ ] 桌面验收：关于页布局、手动点击升级、不同主题/DPI、真实新版发布后的整套应用升级。打包安装环境和安装目录拒绝写入的实际设备场景尚未验收。

官方依据：[GitHub 获取最新发布](https://docs.github.com/en/rest/releases/releases#get-the-latest-release)、[ureq AgentBuilder 2.12.1](https://docs.rs/ureq/2.12.1/ureq/struct.AgentBuilder.html)、[ZIP 4.6.1](https://docs.rs/zip/4.6.1/zip/read/struct.ZipArchive.html)、[GetCurrentPackageFullName](https://learn.microsoft.com/en-us/windows/win32/api/appmodel/nf-appmodel-getcurrentpackagefullname)、[WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject)、[GetBinaryTypeW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-getbinarytypew)。

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
