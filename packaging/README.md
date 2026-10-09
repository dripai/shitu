# ShiTu Microsoft Store MSIX package

`tools/package-store-msix.ps1` packages only ShiTu. It uses `packaging/AppxManifest.xml`, preserves the existing Store identity, replaces `__PACKAGE_VERSION__` with `X.Y.Z.0`, and rejects a mismatched executable name.

```powershell
.\tools\package-store-msix.ps1 -Product ShiTu -ExecutablePath .\target\release\ShiTu.exe -Version 0.3.0 -OutputDirectory .\release-assets
```

Windows SDK `MakeAppx.exe` is required. Output:

- `ShiTu-0.3.0-windows-x64.msix`: unsigned MSIX.
- `ShiTu-0.3.0-store.msixupload`: Partner Center upload archive.

ShiTu declares `runFullTrust`, `systemAIModels`, and the Windows App Runtime dependency for enhanced Windows AI OCR.

The EXE uses GPUI's embedded process manifest (PerMonitorV2 DPI, Windows 10 compatibility, Common Controls 6). `build.rs` adds icons/version metadata only, avoiding duplicate RT_MANIFEST resources. Installed package identity remains in `AppxManifest.xml`; this project does not implement external-location/sparse-package registration. The former standalone identity manifest has been removed. See [Microsoft's identity package requirements](https://learn.microsoft.com/windows/apps/desktop/modernize/grant-identity-to-nonpackaged-apps).

Microsoft Store signs accepted submissions. The unsigned package is not a signed sideloading installer. Local packaging checks do not confirm Store acceptance or installation on other Windows versions.

Official references:

- https://learn.microsoft.com/windows/apps/package-and-deploy/choose-distribution-path
- https://learn.microsoft.com/windows/apps/publish/publish-your-app/msix/upload-app-packages
