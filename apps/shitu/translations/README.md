# 界面语言

支持简体中文（源语言）、英语、日语、韩语、法语、德语、西班牙语、葡萄牙语、俄语、印地语。印度有多种语言，本次提供的是印地语（हिन्दी）。语言选项显示各语言的本地名称，便于在切换后找到原语言。

“跟随系统”匹配 locale 的语言部分，例如 `pt-BR`、`pt-PT` 均使用 `pt`，`hi-IN` 使用 `hi`。未支持的系统语言沿用原有英语默认规则。选择后立即预览，点击保存后写入配置并在重启后保留；预览不修改系统语言或 OCR 识别语言。

## 维护方式

- 翻译文件为 `<locale>/LC_MESSAGES/shitu.po`，`msgid` 是唯一文案标识。应用 UI 使用 `@tr("中文")`，Rust 动态提示使用 `i18n::text("中文")`，不要在调用处保留另一份英文。Slint 原生 LineEdit/TextEdit 菜单沿用框架的 `Cut`、`Copy`、`Paste`、`Select All` 键，`zh` 文件也提供这四项的中文翻译。
- 新增文案时同步更新全部 PO 文件。`build/translations.rs` 使用与 Slint 相同的 `rspolib 0.1.2` 解析器，检查缺失键、空译文、重复键、fuzzy/obsolete 条目和 `{}` 占位符。
- Slint 1.17.0 的公开 API 支持切换内置 UI 翻译，没有按 `msgid` 查询内置翻译的 Rust 接口。因此构建时从相同 PO 文件生成 Rust 静态文本表，后台线程和 OCR 子进程不调用 Slint 私有 API。
- 当前文案均为无 context 的单数条目。需要复数或 context 时，应先同步扩展 Rust 文案接口与校验器。

原生文件对话框的系统按钮及操作系统返回的错误详情保留 Windows 提供的语言。译文尚需母语校对；印地语字形、长文本布局、鼠标、键盘和不同 DPI 下的桌面显示由用户实测。

官方依据：[Slint 1.17.0 内置翻译构建配置](https://docs.rs/slint-build/1.17.0/slint_build/struct.CompilerConfiguration.html#method.with_bundled_translations)、[运行时语言切换](https://docs.rs/slint/1.17.0/slint/fn.select_bundled_translation.html)、[原生 ComboBox](https://releases.slint.dev/1.17.0/docs/slint/reference/std-widgets/combobox/)、[rspolib 0.1.2](https://docs.rs/rspolib/0.1.2/rspolib/)。
