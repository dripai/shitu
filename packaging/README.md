# ShiTu Microsoft Store MSIX package

`tools/package-store-msix.ps1` packages only ShiTu. It uses `packaging/shitu/AppxManifest.xml`, preserves the existing Store identity, replaces `__PACKAGE_VERSION__` with `X.Y.Z.0`, and rejects a mismatched executable name.

```powershell
.\tools\package-store-msix.ps1 -Product ShiTu -ExecutablePath .\target\release\ShiTu.exe -Version 0.2.0 -OutputDirectory .\release-assets
```

Windows SDK `MakeAppx.exe` is required. Output:

- `ShiTu-0.2.0-windows-x64.msix`: unsigned MSIX.
- `ShiTu-0.2.0-store.msixupload`: Partner Center upload archive.

ShiTu declares `runFullTrust`, `systemAIModels`, and the Windows App Runtime dependency for enhanced Windows AI OCR.

Microsoft Store signs accepted submissions. The unsigned package is not a signed sideloading installer. Local packaging checks do not confirm Store acceptance or installation on other Windows versions.

Official references:

- https://learn.microsoft.com/windows/apps/package-and-deploy/choose-distribution-path
- https://learn.microsoft.com/windows/apps/publish/publish-your-app/msix/upload-app-packages
