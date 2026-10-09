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

- [x] `cargo test --offline --locked`：63 项通过，覆盖图像/标注、配置、翻译、快捷键过滤、布局、OCR 转换、缩放、原始像素预览和配置回滚错误。
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
