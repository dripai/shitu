# 界面语言

支持简体中文、英语、日语、韩语、法语、德语、西班牙语、葡萄牙语、俄语和印地语。系统语言按主语言匹配，不支持的系统语言使用英语。选择语言立即预览，保存后在重启时保留；不改变 OCR 识别语言。

- `<locale>/LC_MESSAGES/shitu.po` 是唯一应用文案源；Rust UI 和后台任务均使用 `i18n::text("中文")`。
- `tools/translations.rs` 使用 rspolib 0.1.2，构建时检查键集合、空译文、重复键、fuzzy/obsolete 条目、源码键和占位符，并生成静态文本表。
- GPUI Kit 0.7.1 的 Input/Textarea 使用官方 `context_menu`、`NativeMenu` 和 Cut/Copy/Paste/SelectAll actions；项目只提供现有 PO 文案，编辑权限和选区由官方 action 处理，不重写文本编辑、菜单定位或焦点处理。菜单构造期间不读取正在更新的输入实体，以避免重入借用。框架自身译文没有覆盖全部十种语言，因此不能直接依赖默认编辑菜单。
- 只支持无 context 的单数条目。新增文案须同步更新全部 PO 文件。
- 原生文件对话框和系统错误使用 Windows 提供的语言；译文仍需母语校对，长文本、印地语字形和中文 IME 需要实际设备验证。

依据：[GPUI Kit 0.7.1](https://docs.rs/gpui-kit/0.7.1/gpui_kit/)、[Input::context_menu](https://docs.rs/gpui-component/0.7.1/gpui_component/input/struct.Input.html#method.context_menu)、[rspolib 0.1.2](https://docs.rs/rspolib/0.1.2/rspolib/)。
