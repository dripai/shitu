# PicKeep Microsoft Store MSIX package

`tools/package-store-msix.ps1` packages only PicKeep. It uses `packaging/AppxManifest.xml`, preserves the existing Store identity, replaces `__PACKAGE_VERSION__` with `X.Y.Z.0`, and rejects a mismatched executable name.

中文显示名为“图片馆”，英文品牌和程序名为 `PicKeep` / `PicKeep.exe`。清单的 `Properties/DisplayName` 和 `uap:VisualElements/@DisplayName` 使用“图片馆”；Store 分配的包身份及 GitHub 仓库地址保持不变。

本地清单不直接修改商店页面。仍需在 Partner Center 为现有产品检查并预留“图片馆”，在简体中文商店详情中选择该名称，再提交更新审核。名称可用性及审核结果尚未确认。

官方依据：[清单显示名](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-displayname)、[应用显示名](https://learn.microsoft.com/en-us/uwp/schemas/appxpackage/uapmanifestschema/element-uap-visualelements)、[Store 名称预留](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/reserve-your-apps-name)。

```powershell
cargo build --release --locked --package pickeep --bin PicKeep
.\tools\package-store-msix.ps1 -Product PicKeep -ExecutablePath .\target\release\PicKeep.exe -Version 1.0.0 -OutputDirectory .\release-assets
```

Windows SDK `MakeAppx.exe` is required. Output:

- `PicKeep-1.0.0-windows-x64.msix`: unsigned MSIX; package version `1.0.0.0`.
- `PicKeep-1.0.0-store.msixupload`: Partner Center upload archive.

PicKeep declares `runFullTrust`, `systemAIModels`, and the Windows App Runtime dependency for enhanced Windows AI OCR.

The EXE uses GPUI's embedded process manifest (PerMonitorV2 DPI, Windows 10 compatibility, Common Controls 6). `build.rs` adds icons/version metadata only, avoiding duplicate RT_MANIFEST resources. Installed package identity remains in `AppxManifest.xml`; this project does not implement external-location/sparse-package registration. The former standalone identity manifest has been removed. See [Microsoft's identity package requirements](https://learn.microsoft.com/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps).

Microsoft Store signs accepted submissions. The unsigned package is not a signed sideloading installer. Local packaging checks do not confirm Store acceptance or installation on other Windows versions.

Official references:

- https://learn.microsoft.com/windows/apps/package-and-deploy/choose-distribution-path
- https://learn.microsoft.com/windows/apps/publish/publish-your-app/msix/upload-app-packages
